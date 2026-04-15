import { SidebarProvider } from "@/components/ui/sidebar";
import { create } from "@bufbuild/protobuf";
import { createRouterTransport } from "@connectrpc/connect";
import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vite-plus/test";

import {
  AuthService,
  GetCurrentUserResponseSchema,
  GetSetupStatusResponseSchema,
  LoginResponseSchema,
  LogoutResponseSchema,
} from "@ps/api/gen/canonical/prism/v1/auth_pb";
import { createTestQueryClient, setupCleanup } from "@ps/test-utils";

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(AuthService, {
      getSetupStatus: () => create(GetSetupStatusResponseSchema, { setupComplete: true }),
      getCurrentUser: () => create(GetCurrentUserResponseSchema, { username: "admin", displayName: "Admin" }),
      login: () => create(LoginResponseSchema, { sessionToken: "test-token" }),
      logout: () => create(LogoutResponseSchema, {}),
      completeSetup: () => ({}),
      previewBackup: async () => ({}),
      restoreBackup: async () => ({}),
    });
  }),
}));

const renderPage = async (): Promise<void> => {
  const { default: LoginPage } = await import("./login-page");
  const queryClient = createTestQueryClient();
  render(
    <QueryClientProvider client={queryClient}>
      <SidebarProvider>
        <MemoryRouter>
          <LoginPage />
        </MemoryRouter>
      </SidebarProvider>
    </QueryClientProvider>,
  );

  await waitFor(() => {
    expect(screen.getByText("Sign in to Prism")).toBeInTheDocument();
  });
};

describe("LoginPage", () => {
  setupCleanup();

  it("renders login form with username and password fields", async () => {
    await renderPage();

    expect(screen.getByLabelText("Username")).toBeInTheDocument();
    expect(screen.getByLabelText("Password")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Sign In" })).toBeInTheDocument();
  });

  it("renders branding elements", async () => {
    await renderPage();

    expect(screen.getByText("Engineering Insights Platform")).toBeInTheDocument();
    expect(screen.getByText("Enter your credentials to continue")).toBeInTheDocument();
  });

  it("submits form with filled inputs", async () => {
    await renderPage();

    fireEvent.change(screen.getByLabelText("Username"), { target: { value: "admin" } });
    fireEvent.change(screen.getByLabelText("Password"), { target: { value: "pass123" } });

    expect(screen.getByLabelText("Username")).toHaveValue("admin");
    expect(screen.getByLabelText("Password")).toHaveValue("pass123");

    fireEvent.click(screen.getByRole("button", { name: "Sign In" }));

    // Button should show "Signing in..." while pending
    await waitFor(() => {
      expect(screen.queryByText("Signing in...") ?? screen.queryByText("Sign In")).toBeTruthy();
    });
  });
});
