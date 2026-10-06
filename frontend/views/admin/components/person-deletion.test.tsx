import { create } from "@bufbuild/protobuf";
import { Code, ConnectError, createRouterTransport } from "@connectrpc/connect";
import { QueryClientProvider, type QueryClient } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { ConfigService } from "@ps/api/gen/canonical/prism/v1/config_pb";
import { OrgService, PersonSchema, type DeletePersonRequest } from "@ps/api/gen/canonical/prism/v1/org_pb";
import { createTestQueryClient, setupCleanup } from "@ps/test-utils";

const calls = vi.hoisted(() => ({
  delete: vi.fn<(request: DeletePersonRequest) => void>(),
  close: vi.fn<(open: boolean) => void>(),
  success: vi.fn<(message: string) => void>(),
  error: vi.fn<(message: string) => void>(),
  reject: false,
  pending: undefined as (() => void) | undefined,
  admin: true,
}));
vi.mock("sonner", () => ({ toast: { success: calls.success, error: calls.error } }));
vi.mock("@ps/hooks/use-auth", () => ({
  useCurrentUser: (): { data: { role: string } } => ({ data: { role: calls.admin ? "admin" : "viewer" } }),
}));
vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(ConfigService, { listSources: () => ({ sources: [] }) });
    service(OrgService, {
      deletePerson: async (request) => {
        calls.delete(request);
        if (calls.reject) throw new ConnectError("Deletion failed", Code.Internal);
        if (calls.pending)
          await new Promise<void>((resolve) => {
            calls.pending = resolve;
          });
        return {};
      },
    });
  }),
}));

const renderPerson = async (active = false): Promise<QueryClient> => {
  const { PersonDetailDialog } = await import("@/views/admin/components/person-detail-dialog");
  const client = createTestQueryClient();
  client.setQueryData(["org", "people"], []);
  render(
    <QueryClientProvider client={client}>
      <PersonDetailDialog
        person={create(PersonSchema, { id: "person-1", name: "Alice", active })}
        teams={[]}
        open
        onOpenChange={calls.close}
      />
    </QueryClientProvider>,
  );
  return client;
};

describe("permanent person deletion", () => {
  setupCleanup();
  beforeEach(() => {
    vi.clearAllMocks();
    calls.reject = false;
    calls.pending = undefined;
    calls.admin = true;
  });

  it("requires confirmation and permits cancellation without deleting", async () => {
    await renderPerson();
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    expect(screen.getByText("Permanently delete Alice?")).toBeInTheDocument();
    expect(calls.delete).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Keep person" }));
    expect(screen.queryByRole("button", { name: "Confirm permanent deletion" })).not.toBeInTheDocument();
    expect(calls.delete).not.toHaveBeenCalled();
  });

  it("hides deletion for active people", async () => {
    await renderPerson(true);
    expect(screen.queryByRole("button", { name: "Delete permanently" })).not.toBeInTheDocument();
  });

  it("hides deletion without admin access", async () => {
    calls.admin = false;
    await renderPerson();
    expect(screen.queryByRole("button", { name: "Delete permanently" })).not.toBeInTheDocument();
  });

  it("blocks duplicate submission, invalidates the directory and closes on success", async () => {
    calls.pending = (): void => {};
    const client = await renderPerson();
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm permanent deletion" }));
    expect(await screen.findByRole("button", { name: "Deleting..." })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Keep person" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Deleting..." }));
    expect(calls.delete).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ personId: "person-1" }));
    calls.pending();
    await waitFor(() => expect(calls.close).toHaveBeenCalledWith(false));
    expect(client.getQueryState(["org", "people"])?.isInvalidated).toBe(true);
    expect(calls.success).toHaveBeenCalledWith("Person permanently deleted");
  });

  it("keeps the dialog open and permits retry on failure", async () => {
    calls.reject = true;
    await renderPerson();
    fireEvent.click(screen.getByRole("button", { name: "Delete permanently" }));
    fireEvent.click(screen.getByRole("button", { name: "Confirm permanent deletion" }));
    expect(await screen.findByText(/Deletion failed/)).toBeInTheDocument();
    expect(calls.close).not.toHaveBeenCalled();
    expect(calls.error).toHaveBeenCalled();
    calls.reject = false;
    fireEvent.click(screen.getByRole("button", { name: "Confirm permanent deletion" }));
    await waitFor(() => expect(calls.close).toHaveBeenCalledWith(false));
  });
});
