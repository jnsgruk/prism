import PersonProfilePage from "@/views/people/pages/person-profile-page";
import { QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, within } from "@testing-library/react";
import { cloneElement, type ReactElement } from "react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { describe, expect, it, vi } from "vite-plus/test";

import { createTestQueryClient, setupCleanup } from "@ps/test-utils";

vi.mock("@ps/api/transport", async () => {
  const { createRouterTransport } = await import("@connectrpc/connect");
  const { Platform } = await import("@ps/api/gen/canonical/prism/v1/common_pb");
  const { MetricsService } = await import("@ps/api/gen/canonical/prism/v1/metrics_pb");
  const { InsightsService } = await import("@ps/api/gen/canonical/prism/v1/insights_pb");
  const { OrgService } = await import("@ps/api/gen/canonical/prism/v1/org_pb");

  return {
    transport: createRouterTransport(({ service }) => {
      service(MetricsService, {
        getIndividualProfile: () => ({
          personId: "person-1",
          name: "Alice Smith",
          identities: [
            { platform: Platform.GITHUB, username: "alice-gh" },
            { platform: Platform.JIRA, username: "alice-jira" },
            { platform: Platform.LAUNCHPAD, username: "alice-lp" },
            { platform: Platform.MATTERMOST, username: "alice-mm" },
            { platform: Platform.DISCOURSE, platformInstance: "ubuntu", username: "alice-forum" },
            { platform: Platform.DISCOURSE, platformInstance: "snapcraft", username: "alice-forum" },
          ],
          activityByPlatform: [
            { platform: Platform.GITHUB, contributionCount: 7 },
            { platform: Platform.JIRA, contributionCount: 8 },
            { platform: Platform.DISCOURSE, platformInstance: "ubuntu", contributionCount: 9 },
            { platform: Platform.DISCOURSE, platformInstance: "snapcraft", contributionCount: 10 },
          ],
        }),
        listPersonContributions: () => ({ totalCount: 0 }),
      });
      service(InsightsService, { getPersonInsights: () => ({}) });
      service(OrgService, { listPeople: () => ({ people: [] }) });
    }),
  };
});

vi.mock("@/components/page-header", () => ({
  PageHeader: ({ title }: { title: React.ReactNode }): React.ReactElement => <header>{title}</header>,
}));

// Give the real Recharts chart a fixed viewport in the DOM test environment.
vi.mock("recharts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("recharts")>()),
  ResponsiveContainer: ({ children }: { children: ReactElement<Record<string, unknown>> }): React.ReactElement => (
    <div>{cloneElement(children, { width: 800, height: 250 })}</div>
  ),
}));

setupCleanup();

const renderProfile = (): ReturnType<typeof render> =>
  render(
    <QueryClientProvider client={createTestQueryClient()}>
      <MemoryRouter initialEntries={["/people/person-1"]}>
        <Routes>
          <Route path="/people/:personId" element={<PersonProfilePage />} />
        </Routes>
      </MemoryRouter>
    </QueryClientProvider>,
  );

const platformNames = ["GitHub", "Jira", "Discourse (ubuntu)", "Discourse (snapcraft)"];

describe("person profile platform labels", () => {
  it("shows platform names and distinguishes accounts on two Discourse instances", async () => {
    renderProfile();

    const trigger = await screen.findByRole("button", { name: /Identities/ });
    fireEvent.click(trigger);
    const identities = trigger.closest("[data-slot='card']");
    expect(identities).not.toBeNull();
    if (!(identities instanceof HTMLElement)) throw new Error("identity card missing");

    const accounts = within(identities);
    for (const name of [...platformNames, "Launchpad", "Mattermost"]) {
      expect(accounts.getByText(name)).toBeVisible();
    }

    expect(accounts.getAllByText("alice-forum")).toHaveLength(2);
    for (const value of ["1", "2", "3", "4", "5"]) {
      expect(accounts.queryByText(value)).not.toBeInTheDocument();
    }
  });

  it("labels contribution summaries with platform names and instances", async () => {
    renderProfile();
    await screen.findByText("Activity by platform");

    for (const name of platformNames) {
      const summary = screen.getAllByText(name).find((element) => element.tagName === "P");
      expect(summary).toBeDefined();
      expect(summary).toBeVisible();
    }

    expect(screen.getByText("7 contributions")).toBeVisible();
    expect(screen.getByText("10 contributions")).toBeVisible();
  });
});
