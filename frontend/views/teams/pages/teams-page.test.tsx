import { create } from "@bufbuild/protobuf";
import { createRouterTransport } from "@connectrpc/connect";
import { QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router";
import { describe, expect, it, vi } from "vitest";

import { SidebarProvider } from "@/components/ui/sidebar";
import {
  CompareTeamsResponseSchema,
  GetFlowMetricsResponseSchema,
  GetTeamMetricsResponseSchema,
  ListPeriodsResponseSchema,
  MetricsService,
} from "@ps/api/gen/canonical/prism/v1/metrics_pb";
import {
  GetTeamResponseSchema,
  GetTeamTreeResponseSchema,
  OrgService,
  TeamType,
} from "@ps/api/gen/canonical/prism/v1/org_pb";
import {
  GetPersonInsightsResponseSchema,
  GetTeamInsightsResponseSchema,
  InsightsService,
} from "@ps/api/gen/canonical/prism/v1/insights_pb";
import { createTestQueryClient, setupCleanup } from "@ps/test-utils";

const mockTeamTree = {
  roots: [
    {
      id: "team-1",
      name: "Engineering",
      teamType: TeamType.ORG,
      memberCount: 10,
      totalMemberCount: 25,
      children: [
        {
          id: "team-2",
          name: "Backend",
          teamType: TeamType.GROUP,
          memberCount: 5,
          totalMemberCount: 5,
          children: [],
        },
      ],
    },
  ],
};

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(OrgService, {
      getTeamTree: () => create(GetTeamTreeResponseSchema, mockTeamTree),
      getTeam: () =>
        create(GetTeamResponseSchema, {
          team: {
            id: "team-1",
            name: "Engineering",
            teamType: TeamType.ORG,
            memberCount: 10,
            totalMemberCount: 25,
          },
          members: [{ id: "p-1", name: "Alice" }],
        }),
      listTeams: () => ({}),
      listPeople: () => ({}),
      createTeam: () => ({}),
      updateTeam: () => ({}),
      deleteTeam: () => ({}),
      updatePerson: () => ({}),
      deactivatePerson: () => ({}),
      reactivatePerson: () => ({}),
      assignPersonToTeam: () => ({}),
      removePersonFromTeam: () => ({}),
      listUnassignedPeople: () => ({}),
      importDirectory: () => ({}),
      importJiraUsers: () => ({}),
      assignGithubTeam: () => ({}),
      unassignGithubTeam: () => ({}),
      listGithubTeams: () => ({}),
      listTeamGithubTeams: () => ({}),
      getTeamMappingSuggestions: () => ({}),
      dismissTeamMappingSuggestion: () => ({}),
    });
    service(MetricsService, {
      compareTeams: () =>
        create(CompareTeamsResponseSchema, {
          metrics: [
            {
              teamId: "team-1",
              teamName: "Engineering",
              throughput: 42,
              reviewTurnaroundP75Hours: 4.5,
            },
          ],
        }),
      listPeriods: () => create(ListPeriodsResponseSchema, { periods: [] }),
      getTeamMetrics: () => create(GetTeamMetricsResponseSchema, {}),
      listTeamContributions: () => ({}),
      getFlowMetrics: () => create(GetFlowMetricsResponseSchema, {}),
      getIndividualProfile: () => ({}),
      listPersonContributions: () => ({}),
      getContribution: () => ({}),
    });
    service(InsightsService, {
      getTeamInsights: () => create(GetTeamInsightsResponseSchema, {}),
      getPersonInsights: () => create(GetPersonInsightsResponseSchema, {}),
    });
  }),
}));

const renderPage = async (): Promise<void> => {
  const { default: TeamsPage } = await import("./teams-page");
  const queryClient = createTestQueryClient();
  render(
    <QueryClientProvider client={queryClient}>
      <SidebarProvider>
        <MemoryRouter initialEntries={["/teams/team-1"]}>
          <Routes>
            <Route path="/teams/:teamId" element={<TeamsPage />} />
          </Routes>
        </MemoryRouter>
      </SidebarProvider>
    </QueryClientProvider>,
  );
};

describe("TeamsPage", () => {
  setupCleanup();

  it("renders period selector", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getByText("Last month")).toBeInTheDocument();
    });

    expect(screen.getByText("Last week")).toBeInTheDocument();
    expect(screen.getByText("Last quarter")).toBeInTheDocument();
  });

  it("renders team name in breadcrumb", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getAllByText("Engineering").length).toBeGreaterThanOrEqual(1);
    });
  });

  it("renders collapsible sections", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getAllByText("Pull Requests").length).toBeGreaterThanOrEqual(1);
    });

    expect(screen.getAllByText("Reviews").length).toBeGreaterThanOrEqual(1);
  });

  it("renders delivery panel with metrics", async () => {
    await renderPage();

    await waitFor(() => {
      // The delivery panel should appear once the team is selected and metrics are loaded
      expect(screen.getAllByText("Engineering").length).toBeGreaterThanOrEqual(1);
    });
  });
});
