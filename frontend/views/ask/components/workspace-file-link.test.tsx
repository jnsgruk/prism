import { AppShell } from "@/components/app-shell";
import { SidebarProvider } from "@/components/ui/sidebar";
import { AnswerContent } from "@/views/ask/components/answer-content";
import WorkspaceDownloadPage from "@/views/ask/pages/workspace-download-page";
import LoginPage from "@/views/login/pages/login-page";
import type { MessageInitShape } from "@bufbuild/protobuf";
import { QueryClientProvider } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import { act, cleanup, fireEvent, render, screen, waitFor, type RenderResult } from "@testing-library/react";
import { StrictMode } from "react";
import { MemoryRouter, Route, Routes, useNavigate } from "react-router-dom";
import { afterEach, beforeEach, describe, expect, it, vi } from "vite-plus/test";

import type {
  DownloadWorkspaceFileRequest,
  ResolveWorkspaceFilesRequest,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { WorkspaceFileAvailability } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { createTestQueryClient } from "@ps/test-utils";

const serviceState = vi.hoisted(() => ({
  resolve:
    vi.fn<
      (
        request: ResolveWorkspaceFilesRequest,
      ) => MessageInitShape<
        typeof import("@ps/api/gen/canonical/prism/v1/reasoning_pb").ResolveWorkspaceFilesResponseSchema
      >
    >(),
  image: vi.fn<(request: { conversationId: string; path: string }) => void>(),
  expired: false,
  deleted: false,
  download: vi.fn<(request: DownloadWorkspaceFileRequest) => void>(),
  loggedIn: true,
  gate: null as Promise<void> | null,
  failure: false,
  truncated: false,
  size: 4n,
}));
vi.mock("@ps/api/transport", async () => {
  const { createRouterTransport, ConnectError, Code } = await import("@connectrpc/connect");
  const { AuthService } = await import("@ps/api/gen/canonical/prism/v1/auth_pb");
  const { ReasoningService } = await import("@ps/api/gen/canonical/prism/v1/reasoning_pb");
  return {
    transport: createRouterTransport(({ service }) => {
      service(AuthService, {
        getSetupStatus: () => ({ setupComplete: true }),
        getCurrentUser: () => {
          if (!serviceState.loggedIn) throw new ConnectError("sign in", Code.Unauthenticated);
          return { username: "test", displayName: "Test", userId: "user" };
        },
        login: () => {
          serviceState.loggedIn = true;
          return { sessionToken: "test-token" };
        },
      });
      service(ReasoningService, {
        resolveWorkspaceFiles: (request) => serviceState.resolve(request),
        getWorkspaceFile: (request) => {
          serviceState.image(request);
          return { downloadUrl: "data:image/png;base64,AA==", contentType: "image/png" };
        },
        async *downloadWorkspaceFile(request) {
          serviceState.download(request);
          if (serviceState.expired) throw new ConnectError("expired", Code.Unauthenticated);
          if (serviceState.deleted) throw new ConnectError("deleted", Code.NotFound);
          yield {
            contentType: "application/pdf",
            totalSizeBytes: serviceState.size,
            data: new Uint8Array(serviceState.size ? [1, 2] : []),
          };
          if (serviceState.gate) await serviceState.gate;
          if (serviceState.failure) throw new ConnectError("transfer failed", Code.Unavailable);
          if (serviceState.size && !serviceState.truncated) yield { data: new Uint8Array([3, 4]) };
        },
      });
    }),
  };
});
vi.mock("sonner", () => ({ toast: { error: vi.fn<(message: string) => void>() } }));

const renderContent = (
  content = "[Report](/workspace/Activity_Report_2026.pdf)",
  conversationId: string | undefined = "reported-conversation",
): RenderResult & { client: QueryClient } => {
  const client = createTestQueryClient();
  const view = render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <AnswerContent content={content} conversationId={conversationId} />
      </MemoryRouter>
    </QueryClientProvider>,
  );
  return { ...view, client };
};

beforeEach(() => {
  serviceState.expired = false;
  serviceState.deleted = false;
  serviceState.image.mockReset();
  serviceState.loggedIn = true;
  serviceState.gate = null;
  serviceState.failure = false;
  serviceState.truncated = false;
  serviceState.size = 4n;
  serviceState.download.mockReset();
  serviceState.resolve.mockReset().mockImplementation((request) => ({
    files: request.paths.map((path: string) => ({ path, availability: WorkspaceFileAvailability.AVAILABLE })),
  }));
  vi.stubGlobal(
    "URL",
    Object.assign(URL, {
      createObjectURL: vi.fn<() => string>(() => "blob:test"),
      revokeObjectURL: vi.fn<(url: string) => void>(),
    }),
  );
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  vi.useRealTimers();
});

const NavigationControl = (): React.ReactElement => {
  const navigate = useNavigate();
  return <button onClick={() => navigate("/ask/new-conversation/files/second.pdf")}>Next file</button>;
};

describe("authenticated workspace links", () => {
  it("batches distinct file checks and deduplicates repeated links", async () => {
    const links = Array.from({ length: 50 }, (_, index) => `[File ${index}](/workspace/file-${index}.pdf)`);
    renderContent([...links, "[Again](/workspace/file-0.pdf)"].join(" "));
    await screen.findByRole("link", { name: "File 49" });
    expect(screen.getAllByRole("link")).toHaveLength(51);
    expect(serviceState.resolve).toHaveBeenCalledTimes(1);
    expect(serviceState.resolve.mock.calls[0]?.[0].paths).toHaveLength(50);
  });

  it("splits file verification at the RPC batch limit", async () => {
    renderContent(Array.from({ length: 129 }, (_, index) => `[File ${index}](/workspace/file-${index}.pdf)`).join(" "));
    await screen.findByRole("link", { name: "File 128" });
    expect(serviceState.resolve).toHaveBeenCalledTimes(2);
    expect(serviceState.resolve.mock.calls.map(([request]) => request.paths.length)).toEqual([128, 1]);
    expect(screen.getAllByRole("link")).toHaveLength(129);
  });

  it("verifies the exact persisted filename, deduplicates checks, saves complete bytes and revokes the URL", async () => {
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    renderContent("[Report](/workspace/Activity_Report_2026.pdf) [Again](/workspace/Activity_Report_2026.pdf)");
    expect(screen.queryByRole("link")).toBeNull();
    const link = await screen.findByRole("link", { name: "Report" });
    expect(link.getAttribute("href")).toBe("/ask/reported-conversation/files/Activity_Report_2026.pdf");
    expect(serviceState.resolve).toHaveBeenCalledTimes(1);
    fireEvent.click(link);
    fireEvent.click(link);
    await waitFor(() => expect(click).toHaveBeenCalledTimes(1));
    expect(serviceState.download).toHaveBeenCalledWith(
      expect.objectContaining({
        conversationId: "reported-conversation",
        path: "Activity_Report_2026.pdf",
      }),
    );
    expect(URL.createObjectURL).toHaveBeenCalledWith(expect.objectContaining({ size: 4 }));
    await waitFor(() => expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:test"), { timeout: 2000 });
  });

  it.each(["failure", "truncated"] as const)("never saves partial bytes on %s", async (kind) => {
    serviceState[kind] = true;
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    renderContent();
    fireEvent.click(await screen.findByRole("link", { name: "Report" }));
    await screen.findByRole("link", { name: "Report" });
    await waitFor(() => expect(serviceState.download).toHaveBeenCalledTimes(1));
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(URL.createObjectURL).not.toHaveBeenCalled();
    expect(click).not.toHaveBeenCalled();
  });

  it("does not allocate a download URL after leaving during a transfer", async () => {
    let finish = (): void => {};
    serviceState.gate = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const click = vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    const view = renderContent();
    fireEvent.click(await screen.findByRole("link", { name: "Report" }));
    await waitFor(() => expect(serviceState.download).toHaveBeenCalledTimes(1));
    view.unmount();
    await act(async () => {
      finish();
    });
    await waitFor(() => expect(view.client.isMutating()).toBe(0));
    expect(URL.createObjectURL).not.toHaveBeenCalled();
    expect(click).not.toHaveBeenCalled();
  });

  it("keeps missing files inactive and can verify them after generation finishes", async () => {
    serviceState.resolve.mockImplementation((request) => ({
      files: request.paths.map((path: string) => ({ path, availability: WorkspaceFileAvailability.MISSING })),
    }));
    const { client } = renderContent();
    await screen.findByText(/File unavailable/);
    expect(screen.queryByRole("link")).toBeNull();
    serviceState.resolve.mockImplementation((request) => ({
      files: request.paths.map((path: string) => ({ path, availability: WorkspaceFileAvailability.AVAILABLE })),
    }));
    await act(async () => {
      await client.invalidateQueries({ queryKey: ["conversations", "workspaceResolution", "reported-conversation"] });
    });
    await screen.findByRole("link", { name: "Report" });
  });

  it("offers verification retry on storage failure and blocks references without context", async () => {
    serviceState.resolve.mockImplementation((request) => ({
      files: request.paths.map((path: string) => ({ path, availability: WorkspaceFileAvailability.UNAVAILABLE })),
    }));
    const view = renderContent();
    await screen.findByRole("button", { name: "Retry" });
    view.unmount();
    renderContent("[Report](/workspace/report.pdf)", "");
    expect(screen.getByText(/File unavailable/)).toBeTruthy();
    expect(screen.queryByRole("link")).toBeNull();
  });

  it("keeps remote and app links as navigation and preserves modified-click destinations", async () => {
    renderContent(
      "[Remote](https://example.org/workspace/report.pdf) [Team](/teams/a) [Local](workspace/folder/report%20one.pdf)",
    );
    expect(screen.getByRole("link", { name: "Remote" }).getAttribute("href")).toBe(
      "https://example.org/workspace/report.pdf",
    );
    expect(screen.getByRole("link", { name: "Team" }).getAttribute("href")).toBe("/teams/a");
    const link = await screen.findByRole("link", { name: "Local" });
    fireEvent.click(link, { ctrlKey: true });
    expect(serviceState.download).not.toHaveBeenCalled();
    expect(link.getAttribute("href")).toBe("/ask/reported-conversation/files/folder/report%20one.pdf");
  });

  it("direct routes decode once and transfer once in StrictMode, including empty files", async () => {
    serviceState.size = 0n;
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    render(
      <StrictMode>
        <QueryClientProvider client={createTestQueryClient()}>
          <SidebarProvider>
            <MemoryRouter initialEntries={["/ask/other-conversation/files/nested/100%25%20report.pdf"]}>
              <Routes>
                <Route path="/ask/:conversationId/files/*" element={<WorkspaceDownloadPage />} />
              </Routes>
            </MemoryRouter>
          </SidebarProvider>
        </QueryClientProvider>
      </StrictMode>,
    );
    await screen.findByText("Download complete.");
    expect(serviceState.download).toHaveBeenCalledTimes(1);
    expect(serviceState.download).toHaveBeenCalledWith(
      expect.objectContaining({ conversationId: "other-conversation", path: "nested/100% report.pdf" }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Download again" }));
    await waitFor(() => expect(serviceState.download).toHaveBeenCalledTimes(2));
  });
  it("preserves an unauthenticated direct destination through login without fetching metadata first", async () => {
    serviceState.loggedIn = false;
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <MemoryRouter initialEntries={["/ask/target/files/nested/report.pdf"]}>
          <AppShell>
            <Routes>
              <Route path="/login" element={<LoginPage />} />
              <Route path="/ask/:conversationId/files/*" element={<WorkspaceDownloadPage />} />
            </Routes>
          </AppShell>
        </MemoryRouter>
      </QueryClientProvider>,
    );
    await screen.findByText("Sign in to Prism");
    expect(serviceState.resolve).not.toHaveBeenCalled();
    expect(serviceState.download).not.toHaveBeenCalled();
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "test" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "password" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign In" }));
    await screen.findByText("Download complete.");
    expect(serviceState.download).toHaveBeenCalledWith(
      expect.objectContaining({ conversationId: "target", path: "nested/report.pdf" }),
    );
  });

  it("starts the newly navigated file after an earlier transfer settles", async () => {
    let release: () => void = () => {};
    serviceState.gate = new Promise<void>((resolve) => {
      release = resolve;
    });
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <SidebarProvider>
          <MemoryRouter initialEntries={["/ask/first/files/first.pdf"]}>
            <NavigationControl />
            <Routes>
              <Route path="/ask/:conversationId/files/*" element={<WorkspaceDownloadPage />} />
            </Routes>
          </MemoryRouter>
        </SidebarProvider>
      </QueryClientProvider>,
    );
    await waitFor(() => expect(serviceState.download).toHaveBeenCalledTimes(1));
    fireEvent.click(screen.getByRole("button", { name: "Next file" }));
    await screen.findByText("second.pdf");
    await act(async () => {
      serviceState.gate = null;
      release();
    });
    await waitFor(() => expect(serviceState.download).toHaveBeenCalledTimes(2));
    expect(serviceState.download).toHaveBeenLastCalledWith(
      expect.objectContaining({ conversationId: "new-conversation", path: "second.pdf" }),
    );
  });
  it("keeps generated images on the shared workspace classifier", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn<typeof fetch>().mockResolvedValue(new Response(new Blob([new Uint8Array([1])], { type: "image/png" }))),
    );
    renderContent("![Generated chart](workspace/nested/chart%20one.png)");
    const image = await screen.findByRole("img", { name: "Generated chart" });
    expect(image.getAttribute("src")).toBe("blob:test");
    expect(serviceState.image).toHaveBeenCalledWith(
      expect.objectContaining({ conversationId: "reported-conversation", path: "nested/chart one.png" }),
    );
    vi.unstubAllGlobals();
  });

  it("makes file deletion after verification recoverable without saving bytes", async () => {
    serviceState.deleted = true;
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <SidebarProvider>
          <MemoryRouter initialEntries={["/ask/target/files/report.pdf"]}>
            <Routes>
              <Route path="/ask/:conversationId/files/*" element={<WorkspaceDownloadPage />} />
            </Routes>
          </MemoryRouter>
        </SidebarProvider>
      </QueryClientProvider>,
    );
    await screen.findByText("Download failed. No partial file was saved.");
    expect(URL.createObjectURL).not.toHaveBeenCalled();
    serviceState.deleted = false;
    fireEvent.click(screen.getByRole("button", { name: "Retry download" }));
    await screen.findByText("Download complete.");
  });

  it("preserves the route when the authenticated session expires during transfer", async () => {
    serviceState.expired = true;
    serviceState.loggedIn = true;
    vi.spyOn(HTMLAnchorElement.prototype, "click").mockImplementation(() => {});
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <MemoryRouter initialEntries={["/ask/expired-conversation/files/report.pdf"]}>
          <AppShell>
            <Routes>
              <Route path="/login" element={<LoginPage />} />
              <Route path="/ask/:conversationId/files/*" element={<WorkspaceDownloadPage />} />
            </Routes>
          </AppShell>
        </MemoryRouter>
      </QueryClientProvider>,
    );
    await screen.findByText("Sign in to Prism");
    expect(URL.createObjectURL).not.toHaveBeenCalled();
    serviceState.expired = false;
    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "test" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "password" } });
    fireEvent.click(screen.getByRole("button", { name: "Sign In" }));
    await screen.findByText("Download complete.");
    expect(serviceState.download).toHaveBeenLastCalledWith(
      expect.objectContaining({ conversationId: "expired-conversation", path: "report.pdf" }),
    );
  });
});
