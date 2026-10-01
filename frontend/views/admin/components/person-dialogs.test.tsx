import { create } from "@bufbuild/protobuf";
import { Code, ConnectError, createRouterTransport } from "@connectrpc/connect";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { MemoryRouter } from "react-router";
import { beforeEach, describe, expect, it, vi } from "vite-plus/test";

import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { ConfigService } from "@ps/api/gen/canonical/prism/v1/config_pb";
import {
  OrgService,
  PersonSchema,
  PlatformIdentitySchema,
  TeamSchema,
  type Person,
  type CreatePersonRequest,
  type AddPersonIdentityRequest,
  type UpdatePersonIdentityRequest,
  type RemovePersonIdentityRequest,
  type UpdatePersonRequest,
  type AssignPersonToTeamRequest,
  type RemovePersonFromTeamRequest,
  type LookupJiraAccountsRequest,
} from "@ps/api/gen/canonical/prism/v1/org_pb";
import { createTestQueryClient, setupCleanup } from "@ps/test-utils";

const calls = vi.hoisted(() => ({
  create: vi.fn<(request: CreatePersonRequest) => void>(),
  update: vi.fn<(request: UpdatePersonIdentityRequest) => void>(),
  add: vi.fn<(request: AddPersonIdentityRequest) => void>(),
  remove: vi.fn<(request: RemovePersonIdentityRequest) => void>(),
  profile: vi.fn<(request: UpdatePersonRequest) => void>(),
  assign: vi.fn<(request: AssignPersonToTeamRequest) => void>(),
  unassign: vi.fn<(request: RemovePersonFromTeamRequest) => void>(),
  lookup: vi.fn<(request: LookupJiraAccountsRequest) => void>(),
  close: vi.fn<(value: boolean) => void>(),
  success: vi.fn<(value: string) => void>(),
  error: vi.fn<(value: string) => void>(),
  rejectCreate: false,
  rejectAdd: false,
  rejectUsername: "",
  rejectLookup: false,
  pending: undefined as (() => void) | undefined,
  person: undefined as Person | undefined,
}));
vi.mock("sonner", () => ({ toast: { success: calls.success, error: calls.error } }));
vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(ConfigService, {
      listSources: () => ({
        sources: [
          { id: "jira-source", name: "Jira Cloud", sourceType: Platform.JIRA, enabled: true },
          {
            id: "ubuntu-source",
            name: "Ubuntu",
            sourceType: Platform.DISCOURSE,
            platformInstance: "ubuntu",
            enabled: true,
          },
          {
            id: "snap-source",
            name: "Snapcraft",
            sourceType: Platform.DISCOURSE,
            platformInstance: "snapcraft",
            enabled: true,
          },
        ],
      }),
    });
    service(OrgService, {
      createPerson: async (request) => {
        calls.create(request);

        if (calls.rejectCreate)
          throw new ConnectError("GitHub account is already owned by another person", Code.AlreadyExists);

        if (calls.pending)
          await new Promise<void>((resolve) => {
            calls.pending = resolve;
          });

        return { person: { id: "created", name: request.name } };
      },
      updatePerson: (request) => {
        calls.profile(request);

        calls.person = create(PersonSchema, {
          ...calls.person!,
          name: request.name ?? calls.person!.name,
          email: request.email,
          level: request.level,
        });
        return { person: calls.person };
      },
      addPersonIdentity: (request) => {
        calls.add(request);

        if (calls.rejectAdd || request.identity?.username === calls.rejectUsername)
          throw new ConnectError("Account already owned", Code.AlreadyExists);

        calls.person = create(PersonSchema, {
          ...calls.person!,
          identities: [
            ...calls.person!.identities,
            create(PlatformIdentitySchema, {
              platform: request.identity!.platform,
              username: request.identity!.username,
              platformInstance: request.identity!.platformInstance,
              platformUserId: request.identity!.platformUserId,
              id: `new-${calls.add.mock.calls.length}`,
            }),
          ],
        });
        return { person: calls.person };
      },
      updatePersonIdentity: (request) => {
        calls.update(request);

        calls.person = create(PersonSchema, {
          ...calls.person!,
          identities: calls.person!.identities.map((identity) =>
            identity.id === request.identityId
              ? { ...identity, username: request.username ?? identity.username }
              : identity,
          ),
        });
        return { person: calls.person };
      },
      removePersonIdentity: (request) => {
        calls.remove(request);

        calls.person = create(PersonSchema, {
          ...calls.person!,
          identities: calls.person!.identities.filter((identity) => identity.id !== request.identityId),
        });
        return { person: calls.person };
      },
      assignPersonToTeam: (request) => {
        calls.assign(request);

        return {};
      },
      removePersonFromTeam: (request) => {
        calls.unassign(request);

        return {};
      },
      lookupJiraAccounts: (request) => {
        calls.lookup(request);

        if (calls.rejectLookup) throw new ConnectError("Jira credentials need updating", Code.FailedPrecondition);

        return {
          accounts:
            request.query === "missing"
              ? []
              : [
                  { accountId: "opaque-a", displayName: "Alex Smith", email: "alex@example.com", active: true },
                  { accountId: "opaque-b", displayName: "Alex Smith", active: true },
                ],
        };
      },
      getTeamTree: () => ({ roots: [{ id: "team-1", name: "Platform team", memberCount: 0 }] }),
      listPeople: () => ({ people: [] }),
      listTeamGithubTeams: () => ({ teams: [] }),
      getTeamMappingSuggestions: () => ({ suggestions: [] }),
    });
  }),
}));

const teams = [create(TeamSchema, { id: "team-1", name: "Platform team" })];

const renderAdd = async (): Promise<QueryClient> => {
  const { AddPersonDialog } = await import("@/views/admin/components/add-person-dialog");
  const queryClient = createTestQueryClient();

  render(
    <QueryClientProvider client={queryClient}>
      <AddPersonDialog teams={teams} open onOpenChange={calls.close} />
    </QueryClientProvider>,
  );
  await waitFor(() => expect(queryClient.getQueryData(["config", "sources"])).toBeDefined());
  return queryClient;
};

const fillName = (): void => {
  fireEvent.change(screen.getByLabelText("Name"), { target: { value: "New colleague" } });
};

const choose = async (label: string, option: string): Promise<void> => {
  fireEvent.click(screen.getByRole("combobox", { name: label }));
  const item = await screen.findByRole("option", { name: option });
  fireEvent.pointerDown(item, { pointerType: "mouse" });
  fireEvent.click(item);
  await waitFor(() => expect(screen.getByRole("combobox", { name: label })).not.toHaveTextContent("Choose"));
};

describe("manual person dialogs", () => {
  setupCleanup();

  beforeEach(() => {
    vi.clearAllMocks();
    calls.rejectCreate = false;
    calls.rejectAdd = false;
    calls.rejectUsername = "";
    calls.rejectLookup = false;
    calls.pending = undefined;

    calls.person = create(PersonSchema, {
      id: "p-1",
      name: "Colleague",
      active: true,
      identities: [
        { id: "gh-a", platform: Platform.GITHUB, username: "old-login" },
        { id: "gh-b", platform: Platform.GITHUB, username: "keep-login" },
        { id: "disc", platform: Platform.DISCOURSE, username: "forum-user", platformInstance: "disabled-instance" },
      ],
    });
  });

  it("validates blank names and creates an unassigned person without optional fields", async () => {
    const queryClient = await renderAdd();
    queryClient.setQueryData(["org", "people"], []);
    queryClient.setQueryData(["org", "tree"], {});

    fireEvent.click(screen.getByRole("button", { name: "Create person" }));

    expect(await screen.findByText("Enter a name.")).toBeInTheDocument();
    expect(calls.create).not.toHaveBeenCalled();

    fillName();
    fireEvent.click(screen.getByRole("button", { name: "Create person" }));
    await waitFor(() => {
      expect(calls.error.mock.calls[0]?.[0]).toBeUndefined();
      expect(calls.close).toHaveBeenCalledWith(false);
    });
    expect(calls.create.mock.calls[0]![0]).toMatchObject({ name: "New colleague", identities: [] });
    expect(calls.create.mock.calls[0]![0].teamId).toBeUndefined();
    expect(calls.create.mock.calls[0]![0].email).toBeUndefined();
    expect(queryClient.getQueryState(["org", "people"])?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(["org", "tree"])?.isInvalidated).toBe(true);
    expect(calls.success).toHaveBeenCalledWith("Person created");
  });

  it("defaults to no team when opened from a selected team's empty panel", async () => {
    const { OrgTab } = await import("@/views/admin/components/org-tab");
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <MemoryRouter initialEntries={["/admin?team=team-1"]}>
          <OrgTab />
        </MemoryRouter>
      </QueryClientProvider>,
    );

    fireEvent.click(await screen.findByRole("button", { name: "Add" }));
    fireEvent.click(await screen.findByRole("menuitem", { name: "Add person" }));

    expect(screen.getByRole("combobox", { name: "Team" })).toHaveTextContent("No team");

    fillName();
    fireEvent.click(screen.getByRole("button", { name: "Create person" }));
    await waitFor(() => expect(calls.create).toHaveBeenCalled());
    expect(calls.create.mock.calls[0]![0].teamId).toBeUndefined();
    expect(calls.assign).not.toHaveBeenCalled();
  });

  it("requires deliberate team selection and saves two Discourse instances", async () => {
    await renderAdd();

    fillName();
    fireEvent.change(screen.getByLabelText("Email (optional)"), { target: { value: "new@example.com" } });
    await choose("Team", "Platform team");
    fireEvent.click(screen.getByRole("button", { name: "Add Discourse account" }));
    await choose("Discourse instance", "Ubuntu (ubuntu)");
    fireEvent.change(screen.getByLabelText("Discourse username"), { target: { value: "ubuntu-user" } });
    fireEvent.click(screen.getByRole("button", { name: "Add Discourse account" }));
    const groups = screen.getAllByRole("group", { name: /Discourse/ });
    fireEvent.click(within(groups[1]!).getByRole("combobox", { name: "Discourse instance" }));
    const snapOption = await screen.findByRole("option", { name: "Snapcraft (snapcraft)" });
    fireEvent.pointerDown(snapOption, { pointerType: "mouse" });
    fireEvent.click(snapOption);

    await waitFor(() =>
      expect(within(groups[1]!).getByRole("combobox", { name: "Discourse instance" })).toHaveTextContent("snapcraft"),
    );
    fireEvent.change(within(groups[1]!).getByLabelText("Discourse username"), { target: { value: "snap-user" } });
    fireEvent.click(screen.getByRole("button", { name: "Create person" }));
    await waitFor(() => {
      expect(calls.error.mock.calls[0]?.[0]).toBeUndefined();
      expect(calls.close).toHaveBeenCalledWith(false);
    });
    expect(calls.create.mock.calls[0]![0]).toMatchObject({
      teamId: "team-1",
      email: "new@example.com",
      identities: [
        { username: "ubuntu-user", platformInstance: "ubuntu" },
        { username: "snap-user", platformInstance: "snapcraft" },
      ],
    });
  });

  it("retains entered drafts and inline ownership errors after failed save", async () => {
    calls.rejectCreate = true;
    await renderAdd();

    fillName();
    fireEvent.click(screen.getByRole("button", { name: "Add GitHub account" }));
    fireEvent.change(screen.getByLabelText("GitHub username"), { target: { value: "occupied" } });
    fireEvent.click(screen.getByRole("button", { name: "Create person" }));

    expect(await screen.findByText(/GitHub account is already owned/)).toBeInTheDocument();
    expect(screen.getByLabelText("Name")).toHaveValue("New colleague");
    expect(screen.getByLabelText("GitHub username")).toHaveValue("occupied");
    expect(calls.close).not.toHaveBeenCalled();
    expect(calls.success).not.toHaveBeenCalled();
    expect(calls.error).toHaveBeenCalled();
  });

  it("disables duplicate submission while pending", async () => {
    calls.pending = (): void => {};
    await renderAdd();

    fillName();
    fireEvent.click(screen.getByRole("button", { name: "Create person" }));

    expect(await screen.findByRole("button", { name: "Creating..." })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Creating..." }));
    expect(calls.create).toHaveBeenCalledTimes(1);
    calls.pending();
    await waitFor(() => {
      expect(calls.error.mock.calls[0]?.[0]).toBeUndefined();
      expect(calls.close).toHaveBeenCalledWith(false);
    });
  });

  it("canceling the add form never writes", async () => {
    await renderAdd();

    fillName();
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(calls.error.mock.calls[0]?.[0]).toBeUndefined();
      expect(calls.close).toHaveBeenCalledWith(false);
    });
    expect(calls.create).not.toHaveBeenCalled();
  });

  it("uses saved row IDs to edit/remove accounts without dropping disabled instances", async () => {
    const { PersonDetailDialog } = await import("@/views/admin/components/person-detail-dialog");
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <PersonDetailDialog person={calls.person!} teams={teams} open onOpenChange={calls.close} />
      </QueryClientProvider>,
    );

    fireEvent.change(screen.getAllByLabelText("GitHub username")[0]!, { target: { value: "correct-login" } });

    expect(screen.getByRole("combobox", { name: "Discourse instance" })).toHaveTextContent("disabled-instance");
    fireEvent.click(screen.getByRole("button", { name: "Remove account 2" }));
    fireEvent.click(screen.getByRole("button", { name: "Add GitHub account" }));
    fireEvent.change(screen.getAllByLabelText("GitHub username")[1]!, { target: { value: "new-login" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => {
      expect(calls.error.mock.calls[0]?.[0]).toBeUndefined();
      expect(calls.close).toHaveBeenCalledWith(false);
    });
    expect(calls.update.mock.calls[0]![0]).toMatchObject({ identityId: "gh-a", username: "correct-login" });
    expect(calls.remove.mock.calls[0]![0]).toMatchObject({ identityId: "gh-b" });
    expect(calls.person?.identities.map((identity) => identity.username)).toEqual([
      "correct-login",
      "forum-user",
      "new-login",
    ]);
  });

  it("explicitly selects between duplicate Jira names and saves account ID separately", async () => {
    await renderAdd();

    fillName();
    fireEvent.click(screen.getByRole("button", { name: "Add Jira account" }));
    await choose("Jira lookup source", "Jira Cloud");
    fireEvent.change(screen.getByLabelText("Search Jira accounts"), { target: { value: "Alex" } });
    const candidate = await screen.findByRole("button", { name: /Alex Smith Email hidden · opaque-b/ });

    expect(screen.getByLabelText("Jira account ID (opaque ID, not email)")).toHaveValue("");
    fireEvent.click(candidate);
    fireEvent.click(screen.getByRole("button", { name: "Create person" }));
    await waitFor(() => {
      expect(calls.error.mock.calls[0]?.[0]).toBeUndefined();
      expect(calls.close).toHaveBeenCalledWith(false);
    });
    expect(calls.create.mock.calls[0]![0].identities[0]).toMatchObject({
      username: "Alex Smith",
      platformUserId: "opaque-b",
    });
    expect(calls.lookup.mock.calls[0]![0]).toMatchObject({ sourceId: "jira-source", query: "Alex" });
  });

  it("shows Jira no-results/errors and permits direct opaque account ID entry", async () => {
    await renderAdd();

    fillName();
    fireEvent.click(screen.getByRole("button", { name: "Add Jira account" }));
    await choose("Jira lookup source", "Jira Cloud");
    fireEvent.change(screen.getByLabelText("Search Jira accounts"), { target: { value: "missing" } });

    expect(await screen.findByText(/No matching accounts/)).toBeInTheDocument();
    calls.rejectLookup = true;
    fireEvent.change(screen.getByLabelText("Search Jira accounts"), { target: { value: "other" } });
    expect(await screen.findByText(/Jira credentials need updating/)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("Jira account ID (opaque ID, not email)"), {
      target: { value: "direct-opaque" },
    });
    fireEvent.change(screen.getByLabelText("Jira display name / username"), { target: { value: "Other colleague" } });
    fireEvent.click(screen.getByRole("button", { name: "Create person" }));
    await waitFor(() => {
      expect(calls.error.mock.calls[0]?.[0]).toBeUndefined();
      expect(calls.close).toHaveBeenCalledWith(false);
    });
    expect(calls.create.mock.calls[0]![0].identities[0]).toMatchObject({
      username: "Other colleague",
      platformUserId: "direct-opaque",
    });
  });

  it("retries a partially saved account edit without duplicating earlier successful additions", async () => {
    const { PersonDetailDialog } = await import("@/views/admin/components/person-detail-dialog");
    const queryClient = createTestQueryClient();
    queryClient.setQueryData(["org", "people"], []);
    queryClient.setQueryData(["org", "tree"], {});
    render(
      <QueryClientProvider client={queryClient}>
        <PersonDetailDialog person={calls.person!} teams={teams} open onOpenChange={calls.close} />
      </QueryClientProvider>,
    );

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Corrected colleague" } });
    fireEvent.click(screen.getByRole("button", { name: "Add GitHub account" }));
    fireEvent.change(screen.getAllByLabelText("GitHub username")[2]!, { target: { value: "first-new" } });
    fireEvent.click(screen.getByRole("button", { name: "Add GitHub account" }));
    fireEvent.change(screen.getAllByLabelText("GitHub username")[3]!, { target: { value: "second-new" } });
    calls.rejectUsername = "second-new";
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText(/Account already owned/)).toBeInTheDocument();
    expect(calls.close).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Name")).toHaveValue("Corrected colleague");
    expect(calls.add).toHaveBeenCalledTimes(2);
    expect(queryClient.getQueryState(["org", "people"])?.isInvalidated).toBe(true);

    calls.rejectUsername = "";
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() => expect(calls.close).toHaveBeenCalledWith(false));
    expect(calls.profile).toHaveBeenCalledTimes(1);
    expect(calls.add).toHaveBeenCalledTimes(3);
    expect(calls.add.mock.calls.map(([request]) => request.identity?.username)).toEqual([
      "first-new",
      "second-new",
      "second-new",
    ]);
    expect(calls.person?.identities.filter((identity) => identity.username === "first-new")).toHaveLength(1);
    expect(calls.success).toHaveBeenCalledTimes(1);
  });

  it("preserves a legacy imported Jira row without an account ID when editing personal details", async () => {
    calls.person = create(PersonSchema, {
      ...calls.person!,
      identities: [
        ...calls.person!.identities,
        create(PlatformIdentitySchema, { id: "legacy-jira", platform: Platform.JIRA, username: "imported-account" }),
      ],
    });
    const { PersonDetailDialog } = await import("@/views/admin/components/person-detail-dialog");
    render(
      <QueryClientProvider client={createTestQueryClient()}>
        <PersonDetailDialog person={calls.person} teams={teams} open onOpenChange={calls.close} />
      </QueryClientProvider>,
    );

    fireEvent.change(screen.getByLabelText("Name"), { target: { value: "Corrected colleague" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(calls.close).toHaveBeenCalledWith(false));
    expect(calls.update).not.toHaveBeenCalled();
    expect(calls.remove).not.toHaveBeenCalled();
    expect(calls.person?.identities.find((identity) => identity.id === "legacy-jira")?.platformUserId).toBeUndefined();
  });

  it("supports keyboard dialog dismissal without saving a draft", async () => {
    await renderAdd();

    fillName();
    fireEvent.keyDown(screen.getByLabelText("Name"), { key: "Escape", code: "Escape" });

    await waitFor(() => expect(calls.close).toHaveBeenCalledWith(false));
    expect(calls.create).not.toHaveBeenCalled();
  });
});
