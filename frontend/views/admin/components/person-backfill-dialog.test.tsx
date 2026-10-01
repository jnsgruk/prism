import { create } from "@bufbuild/protobuf";
import { Code, ConnectError, createRouterTransport } from "@connectrpc/connect";
import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { AuthService } from "@ps/api/gen/canonical/prism/v1/auth_pb";
import { Platform, RunStatus } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { ConfigService } from "@ps/api/gen/canonical/prism/v1/config_pb";
import {
  HandlersService,
  PipelineInfoSchema,
  type TriggerPipelineRequest,
  type GetPipelineStatusRequest,
  type PipelineRunSummary,
} from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { PersonSchema } from "@ps/api/gen/canonical/prism/v1/org_pb";
import { createTestQueryClient, setupCleanup } from "@ps/test-utils";

const calls = vi.hoisted(() => ({
  launch: vi.fn<(request: TriggerPipelineRequest) => void>(),
  status: vi.fn<(request: GetPipelineStatusRequest) => void>(),
  cancel: vi.fn<(request: { pipelineId: string }) => void>(),
  success: vi.fn<(message: string) => void>(),
  error: vi.fn<(message: string) => void>(),
  close: vi.fn<(open: boolean) => void>(),
  available: true,
  reject: false,
  pending: false,
  role: "admin",
  resolve: undefined as (() => void) | undefined,
  summaries: [] as PipelineRunSummary[],
  pipelineStatus: "pending",
  statusUnavailable: false,
  leak: false,
}));

vi.mock("sonner", () => ({ toast: { success: calls.success, error: calls.error } }));
vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(AuthService, { getCurrentUser: () => ({ role: calls.role }) });
    service(ConfigService, {
      listSources: () => ({
        sources: [
          { id: "github", name: "GitHub org", sourceType: Platform.GITHUB, enabled: true },
          { id: "ubuntu", name: "Ubuntu", sourceType: Platform.DISCOURSE, platformInstance: "ubuntu", enabled: true },
          {
            id: "snapcraft",
            name: "Snapcraft",
            sourceType: Platform.DISCOURSE,
            platformInstance: "snapcraft",
            enabled: true,
          },
          { id: "disabled", name: "Disabled GitHub", sourceType: Platform.GITHUB, enabled: false },
          { id: "jira", name: "Jira", sourceType: Platform.JIRA, enabled: true },
        ],
      }),
    });
    service(HandlersService, {
      getStatus: () => ({
        personBackfillCapabilities: {
          enabled: calls.available,
          reason: "Person backfills await adapter safety validation.",
        },
      }),
      listPipelineRuns: () => ({ pipelines: calls.summaries }),
      triggerPipeline: async (request) => {
        calls.launch(request);
        if (calls.reject) throw new ConnectError("Another pipeline is active", Code.FailedPrecondition);
        if (calls.pending)
          await new Promise<void>((resolve) => {
            calls.resolve = resolve;
          });
        return { pipelineId: "owned" };
      },
      getPipelineStatus: (request) => {
        calls.status(request);
        if (calls.statusUnavailable) throw new ConnectError("Progress temporarily unavailable", Code.Unavailable);
        return {
          current: {
            id: request.pipelineId,
            personId: calls.leak ? "another-person" : request.personId,
            status: calls.pipelineStatus,
            currentStage: "ingestion",
          },
        };
      },
      cancelPipeline: (request) => {
        calls.cancel(request);
        calls.pipelineStatus = "cancelling";
        return {};
      },
    });
  }),
}));

const person = create(PersonSchema, {
  id: "saved-person",
  name: "Alex",
  active: true,
  identities: [
    { id: "gh-account", platform: Platform.GITHUB, username: "alex" },
    { id: "forum-account", platform: Platform.DISCOURSE, platformInstance: "ubuntu", username: "alex-forum" },
  ],
});

const openDialog = async (value = person): Promise<void> => {
  const { PersonBackfillDialog } = await import("./person-backfill-dialog");
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <PersonBackfillDialog person={value} open onOpenChange={calls.close} />
    </QueryClientProvider>,
  );
  await screen.findByRole("checkbox", { name: "GitHub org" });
  await waitFor(() => expect(screen.queryByText("Checking backfill availability…")).not.toBeInTheDocument());
};

const fill = (): void => {
  fireEvent.click(screen.getByRole("checkbox", { name: "GitHub org" }));
  fireEvent.click(screen.getByRole("checkbox", { name: "Ubuntu (ubuntu)" }));
  fireEvent.change(screen.getByLabelText("Start date"), { target: { value: "2024-01-01" } });
};

describe("person backfill dialog", () => {
  setupCleanup();
  beforeEach(() => {
    vi.clearAllMocks();
    calls.available = true;
    calls.reject = false;
    calls.pending = false;
    calls.resolve = undefined;
    calls.summaries = [];
    calls.pipelineStatus = "pending";
    calls.statusUnavailable = false;
    calls.leak = false;
    calls.role = "admin";
  });

  it("selects exact eligible sources and polls the accepted ID", async () => {
    await openDialog();
    expect(screen.getByRole("checkbox", { name: "Snapcraft (snapcraft)" })).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("checkbox", { name: "Disabled GitHub" })).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByRole("checkbox", { name: "Jira" })).toHaveAttribute("aria-disabled", "true");
    fill();
    fireEvent.click(screen.getByRole("button", { name: "Start backfill" }));
    await screen.findByText("Pipeline owned");
    expect(calls.launch.mock.calls[0]![0]).toMatchObject({
      scope: { personId: "saved-person", sourceIds: ["github", "ubuntu"] },
      sinceDate: "2024-01-01",
    });
    expect(calls.launch.mock.calls[0]![0].submissionId).toMatch(/^[0-9a-f-]{36}$/);
    expect(calls.status).toHaveBeenCalledWith(
      expect.objectContaining({ pipelineId: "owned", personId: "saved-person" }),
    );
    expect(calls.success).toHaveBeenCalledWith("Backfill accepted");
    expect(screen.getByRole("button", { name: "Start backfill" })).toBeDisabled();
  });

  it("retains values and submission ID on retry after actionable errors", async () => {
    calls.reject = true;
    await openDialog();
    fill();
    fireEvent.click(screen.getByRole("button", { name: "Start backfill" }));
    await screen.findByText(/Another pipeline is active/);
    expect(screen.getByLabelText("Start date")).toHaveValue("2024-01-01");
    expect(screen.getByRole("checkbox", { name: "Ubuntu (ubuntu)" })).toBeChecked();
    calls.reject = false;
    fireEvent.click(screen.getByRole("button", { name: "Start backfill" }));
    await screen.findByText("Pipeline owned");
    expect(calls.launch.mock.calls[1]![0].submissionId).toBe(calls.launch.mock.calls[0]![0].submissionId);
  });

  it("keeps an accepted submission disabled while its status is unavailable", async () => {
    calls.statusUnavailable = true;
    await openDialog();
    fill();
    fireEvent.click(screen.getByRole("button", { name: "Start backfill" }));
    await screen.findByText(/Progress temporarily unavailable/);
    const submit = screen.getByRole("button", { name: "Start backfill" });
    expect(submit).toBeDisabled();
    fireEvent.click(submit);
    expect(calls.launch).toHaveBeenCalledTimes(1);
  });

  it("disables duplicate submission and prevents future dates", async () => {
    calls.pending = true;
    await openDialog();
    fill();
    fireEvent.change(screen.getByLabelText("Start date"), { target: { value: "2999-01-01" } });
    expect(screen.getByRole("button", { name: "Start backfill" })).toBeDisabled();
    fireEvent.change(screen.getByLabelText("Start date"), { target: { value: "2024-01-01" } });
    fireEvent.click(screen.getByRole("button", { name: "Start backfill" }));
    expect(await screen.findByRole("button", { name: "Submitting…" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Submitting…" }));
    await waitFor(() => expect(calls.launch).toHaveBeenCalledTimes(1));
    calls.resolve!();
    await screen.findByText("Pipeline owned");
  });

  it("shows the server release gate and prevents launching", async () => {
    calls.available = false;
    await openDialog();
    fill();
    expect(await screen.findByText(/await adapter safety validation/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start backfill" })).toBeDisabled();
    expect(calls.launch).not.toHaveBeenCalled();
  });

  it("recovers owned history with rate-limit progress and truthful cancellation", async () => {
    calls.pipelineStatus = "running";
    calls.summaries = [
      {
        $typeName: "canonical.prism.v1.PipelineRunSummary",
        pipeline: create(PipelineInfoSchema, {
          id: "owned",
          personId: person.id,
          status: "running",
          sinceDate: "2024-01-01",
        }),
        runs: [
          {
            $typeName: "canonical.prism.v1.HandlerRun",
            id: "child",
            sourceName: "GitHub org",
            handlerName: "GithubIngestionHandler",
            handlerMethod: "run_scoped",
            status: RunStatus.RUNNING,
            itemsCollected: 7,
            rateLimitWaitsSeconds: 0,
            scopeKind: "person",
            selectedSourceIds: ["github"],
            progressJson: JSON.stringify({
              status_message: "Collecting",
              rate_limit_reset_at: new Date(Date.now() + 600000).toISOString(),
            }),
          },
        ],
      },
    ];
    await openDialog();
    await screen.findByText(/Paused — resumes/);
    fireEvent.click(screen.getByRole("button", { name: "Cancel backfill" }));
    expect(await screen.findByRole("button", { name: "Cancellation requested" })).toBeDisabled();
    expect(calls.cancel).toHaveBeenCalledWith(expect.objectContaining({ pipelineId: "owned" }));
  });

  it("filters another person's history and never polls their pipeline", async () => {
    calls.summaries = [
      {
        $typeName: "canonical.prism.v1.PipelineRunSummary",
        pipeline: create(PipelineInfoSchema, { id: "unrelated", personId: "other", status: "running" }),
        runs: [],
      },
    ];
    await openDialog();
    expect(screen.queryByText("Pipeline unrelated")).not.toBeInTheDocument();
    expect(calls.status).not.toHaveBeenCalled();
  });

  it("explains partial source coverage and exposes the failed selected project", async () => {
    calls.pipelineStatus = "failed";
    calls.summaries = [
      {
        $typeName: "canonical.prism.v1.PipelineRunSummary",
        pipeline: create(PipelineInfoSchema, { id: "partial", personId: person.id, status: "failed" }),
        runs: [
          {
            $typeName: "canonical.prism.v1.HandlerRun",
            id: "partial-source",
            sourceName: "Selected Jira",
            handlerName: "JiraIngestionHandler",
            handlerMethod: "run_scoped",
            status: RunStatus.COMPLETED_WITH_WARNINGS,
            itemsCollected: 4,
            rateLimitWaitsSeconds: 0,
            scopeKind: "person",
            selectedSourceIds: ["jira"],
            errorMessage: "Selected person history is incomplete",
            progressJson: JSON.stringify({
              coverage: ["visible_activity_only: current assignee, visible to the API user"],
              failed_items: [{ key: "PRIVATE", error: "Project is inaccessible to the configured account" }],
            }),
          },
        ],
      },
    ];

    await openDialog();
    expect(await screen.findByText("Selected person history is incomplete")).toBeInTheDocument();
    expect(screen.getByText("Visible activity only: current assignee, visible to the API user")).toBeInTheDocument();
    expect(screen.getByText("4 items")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "1 item could not be collected" }));
    expect(await screen.findByText("PRIVATE")).toBeInTheDocument();
    expect(screen.getByText("Project is inaccessible to the configured account")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Cancel backfill" })).not.toBeInTheDocument();
  });

  it.each(["completed", "completed_with_warnings", "failed", "cancelled"])(
    "recovers a %s run without offering cancellation",
    async (outcome) => {
      calls.pipelineStatus = outcome;
      calls.summaries = [
        {
          $typeName: "canonical.prism.v1.PipelineRunSummary",
          pipeline: create(PipelineInfoSchema, { id: "terminal", personId: person.id, status: outcome }),
          runs: [],
        },
      ];
      await openDialog();
      await screen.findByText("Pipeline terminal");
      expect(screen.queryByRole("button", { name: "Cancel backfill" })).not.toBeInTheDocument();
      expect(screen.getAllByText(outcome.replace(/_/g, " ")).length).toBeGreaterThan(0);
    },
  );

  it("keeps status isolated when switching people", async () => {
    calls.summaries = [
      {
        $typeName: "canonical.prism.v1.PipelineRunSummary",
        pipeline: create(PipelineInfoSchema, { id: "first", personId: person.id, status: "running" }),
        runs: [],
      },
    ];
    const { PersonBackfillDialog } = await import("./person-backfill-dialog");
    const queryClient = createTestQueryClient();
    const view = render(
      <QueryClientProvider client={queryClient}>
        <PersonBackfillDialog key={person.id} person={person} open onOpenChange={calls.close} />
      </QueryClientProvider>,
    );
    await screen.findByText("Pipeline first");
    const another = create(PersonSchema, { ...person, id: "second", name: "Sam" });
    calls.summaries = [];
    view.rerender(
      <QueryClientProvider client={queryClient}>
        <PersonBackfillDialog key={another.id} person={another} open onOpenChange={calls.close} />
      </QueryClientProvider>,
    );
    await screen.findByText("Backfill activity for Sam");
    expect(screen.queryByText("Pipeline first")).not.toBeInTheDocument();
    expect(queryClient.getQueryData(["pipeline", "person", person.id, "history"])).toBeDefined();
    expect(calls.status.mock.calls.every(([request]) => request.personId === person.id)).toBe(true);
  });

  it("hides the person mutation for non-admin users", async () => {
    calls.role = "viewer";
    const { PersonDetailDialog } = await import("./person-detail-dialog");
    const queryClient = createTestQueryClient();
    render(
      <QueryClientProvider client={queryClient}>
        <PersonDetailDialog person={person} teams={[]} open onOpenChange={calls.close} />
      </QueryClientProvider>,
    );
    await waitFor(() => expect(queryClient.getQueryData(["auth", "currentUser"])).toBeDefined());
    expect(screen.queryByRole("button", { name: "Backfill activity" })).not.toBeInTheDocument();
  });
});
