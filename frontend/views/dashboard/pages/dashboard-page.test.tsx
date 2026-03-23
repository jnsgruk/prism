import { create } from "@bufbuild/protobuf";
import { createRouterTransport } from "@connectrpc/connect";
import { render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import { ConfigService, ListSourcesResponseSchema } from "@ps/api/gen/prism/v1/config_pb";
import {
  GetOrgInsightsResponseSchema,
  GetPersonInsightsResponseSchema,
  GetTeamInsightsResponseSchema,
  InsightsService,
} from "@ps/api/gen/prism/v1/insights_pb";
import {
  CompareTeamsResponseSchema,
  GetFlowMetricsResponseSchema,
  GetTeamMetricsResponseSchema,
  ListPeriodsResponseSchema,
  MetricsService,
} from "@ps/api/gen/prism/v1/metrics_pb";
import { GetTeamTreeResponseSchema, OrgService } from "@ps/api/gen/prism/v1/org_pb";
import { createTestQueryClient, setupCleanup } from "@ps/test-utils";

import { SidebarProvider } from "@/components/ui/sidebar";
import { QueryClientProvider } from "@tanstack/react-query";

const noSourcesTransport = createRouterTransport(({ service }) => {
  service(ConfigService, {
    listSources: () => create(ListSourcesResponseSchema, { sources: [] }),
    getSource: () => ({}),
    createSource: () => ({}),
    updateSource: () => ({}),
    deleteSource: () => ({}),
    setSecret: () => ({}),
    testConnection: () => ({}),
  });
  service(OrgService, {
    getTeamTree: () => create(GetTeamTreeResponseSchema, { roots: [] }),
    listTeams: () => ({}),
    getTeam: () => ({}),
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
    compareTeams: () => create(CompareTeamsResponseSchema, { metrics: [] }),
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
    getOrgInsights: () => create(GetOrgInsightsResponseSchema, {}),
  });
});

vi.mock("@ps/api/transport", () => ({
  transport: noSourcesTransport,
}));

const renderPage = async (): Promise<void> => {
  const { default: DashboardPage } = await import("./dashboard-page");
  const queryClient = createTestQueryClient();
  render(
    <QueryClientProvider client={queryClient}>
      <SidebarProvider>
        <MemoryRouter>
          <DashboardPage />
        </MemoryRouter>
      </SidebarProvider>
    </QueryClientProvider>,
  );
};

describe("DashboardPage", () => {
  setupCleanup();

  it("renders page header", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getAllByText("Dashboard").length).toBeGreaterThanOrEqual(1);
    });
  });

  it("shows onboarding state when no sources configured", async () => {
    await renderPage();

    await waitFor(() => {
      expect(screen.getByText("Get started with Prism")).toBeInTheDocument();
    });

    expect(screen.getByText("Configure Sources")).toBeInTheDocument();
  });
});
