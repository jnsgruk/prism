import { create } from "@bufbuild/protobuf";
import { createRouterTransport } from "@connectrpc/connect";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  AdminService,
  CreateApiTokenResponseSchema,
  ListApiTokensResponseSchema,
  ResetDataResponseSchema,
  RevokeApiTokenResponseSchema,
} from "@ps/api/gen/canonical/prism/v1/admin_pb";
import {
  AssignGithubTeamResponseSchema,
  AssignPersonToTeamResponseSchema,
  CreateTeamResponseSchema,
  DeactivatePersonResponseSchema,
  DeleteTeamResponseSchema,
  DismissTeamMappingSuggestionResponseSchema,
  ImportDirectoryResponseSchema,
  ImportJiraUsersResponseSchema,
  ListUnassignedPeopleResponseSchema,
  OrgService,
  ReactivatePersonResponseSchema,
  RemovePersonFromTeamResponseSchema,
  UnassignGithubTeamResponseSchema,
  UpdatePersonResponseSchema,
  UpdateTeamResponseSchema,
} from "@ps/api/gen/canonical/prism/v1/org_pb";
import { TestWrapper } from "@ps/test-utils";

const mockTokens = [{ tokenId: "tok-1", name: "CI Token" }];

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(AdminService, {
      listApiTokens: () => create(ListApiTokensResponseSchema, { tokens: mockTokens }),
      createApiToken: () => create(CreateApiTokenResponseSchema, { token: "new-token-value" }),
      revokeApiToken: () => create(RevokeApiTokenResponseSchema, {}),
      resetData: () => create(ResetDataResponseSchema, {}),
      // eslint-disable-next-line @typescript-eslint/no-empty-function
      async *createBackup() {},
    });
    service(OrgService, {
      importDirectory: () =>
        create(ImportDirectoryResponseSchema, { peopleImported: 5, teamsCreated: 2 }),
      importJiraUsers: () =>
        create(ImportJiraUsersResponseSchema, { identitiesMapped: 3, unmatchedUsers: 1 }),
      createTeam: () => create(CreateTeamResponseSchema, {}),
      updateTeam: () => create(UpdateTeamResponseSchema, {}),
      deleteTeam: () => create(DeleteTeamResponseSchema, {}),
      updatePerson: () => create(UpdatePersonResponseSchema, {}),
      deactivatePerson: () => create(DeactivatePersonResponseSchema, {}),
      reactivatePerson: () => create(ReactivatePersonResponseSchema, {}),
      assignPersonToTeam: () => create(AssignPersonToTeamResponseSchema, {}),
      removePersonFromTeam: () => create(RemovePersonFromTeamResponseSchema, {}),
      listUnassignedPeople: () =>
        create(ListUnassignedPeopleResponseSchema, {
          people: [{ id: "p-1", name: "Unassigned User" }],
        }),
      assignGithubTeam: () => create(AssignGithubTeamResponseSchema, {}),
      unassignGithubTeam: () => create(UnassignGithubTeamResponseSchema, {}),
      dismissTeamMappingSuggestion: () => create(DismissTeamMappingSuggestionResponseSchema, {}),
      // Stubs for methods that OrgService exposes but use-admin doesn't call
      listTeams: () => ({}),
      getTeam: () => ({}),
      getTeamTree: () => ({}),
      listPeople: () => ({}),
      listGithubTeams: () => ({}),
      listTeamGithubTeams: () => ({}),
      getTeamMappingSuggestions: () => ({}),
    });
  }),
}));

describe("admin hooks", () => {
  describe("useListApiTokens", () => {
    it("fetches API tokens", async () => {
      const { useListApiTokens } = await import("./use-admin");
      const { result } = renderHook(() => useListApiTokens(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toHaveLength(1);
      expect(result.current.data?.[0]?.name).toBe("CI Token");
    });
  });

  describe("useCreateApiToken", () => {
    it("creates a token and succeeds", async () => {
      const { useCreateApiToken } = await import("./use-admin");
      const { result } = renderHook(() => useCreateApiToken(), { wrapper: TestWrapper });

      result.current.mutate("deploy-token");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.token).toBe("new-token-value");
    });
  });

  describe("useRevokeApiToken", () => {
    it("revokes a token and succeeds", async () => {
      const { useRevokeApiToken } = await import("./use-admin");
      const { result } = renderHook(() => useRevokeApiToken(), { wrapper: TestWrapper });

      result.current.mutate("tok-1");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useResetData", () => {
    it("resets data and succeeds", async () => {
      const { useResetData } = await import("./use-admin");
      const { result } = renderHook(() => useResetData(), { wrapper: TestWrapper });

      result.current.mutate();

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useImportDirectory", () => {
    it("imports directory file and succeeds", async () => {
      const { useImportDirectory } = await import("./use-admin");
      const { result } = renderHook(() => useImportDirectory(), { wrapper: TestWrapper });

      result.current.mutate(new Uint8Array([1, 2, 3]));

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.peopleImported).toBe(5);
    });
  });

  describe("useImportJiraUsers", () => {
    it("imports Jira users and succeeds", async () => {
      const { useImportJiraUsers } = await import("./use-admin");
      const { result } = renderHook(() => useImportJiraUsers(), { wrapper: TestWrapper });

      result.current.mutate({
        fileContent: new Uint8Array([1, 2, 3]),
        sourceName: "jira-main",
      });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.identitiesMapped).toBe(3);
    });
  });

  describe("useCreateTeam", () => {
    it("creates a team and succeeds", async () => {
      const { useCreateTeam } = await import("./use-admin");
      const { result } = renderHook(() => useCreateTeam(), { wrapper: TestWrapper });
      const { TeamType } = await import("@ps/api/gen/canonical/prism/v1/org_pb");

      result.current.mutate({
        name: "Backend Team",
        teamType: TeamType.GROUP,
        orgName: "Canonical",
      });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useUpdateTeam", () => {
    it("updates a team and succeeds", async () => {
      const { useUpdateTeam } = await import("./use-admin");
      const { result } = renderHook(() => useUpdateTeam(), { wrapper: TestWrapper });

      result.current.mutate({ teamId: "team-1", name: "Renamed Team" });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useDeleteTeam", () => {
    it("deletes a team and succeeds", async () => {
      const { useDeleteTeam } = await import("./use-admin");
      const { result } = renderHook(() => useDeleteTeam(), { wrapper: TestWrapper });

      result.current.mutate("team-1");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useUpdatePerson", () => {
    it("updates a person and succeeds", async () => {
      const { useUpdatePerson } = await import("./use-admin");
      const { result } = renderHook(() => useUpdatePerson(), { wrapper: TestWrapper });

      result.current.mutate({ personId: "p-1", name: "New Name" });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useDeactivatePerson", () => {
    it("deactivates a person and succeeds", async () => {
      const { useDeactivatePerson } = await import("./use-admin");
      const { result } = renderHook(() => useDeactivatePerson(), { wrapper: TestWrapper });

      result.current.mutate("p-1");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useReactivatePerson", () => {
    it("reactivates a person and succeeds", async () => {
      const { useReactivatePerson } = await import("./use-admin");
      const { result } = renderHook(() => useReactivatePerson(), { wrapper: TestWrapper });

      result.current.mutate("p-1");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useAssignPersonToTeam", () => {
    it("assigns a person to a team", async () => {
      const { useAssignPersonToTeam } = await import("./use-admin");
      const { result } = renderHook(() => useAssignPersonToTeam(), { wrapper: TestWrapper });

      result.current.mutate({ personId: "p-1", teamId: "team-1" });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useRemovePersonFromTeam", () => {
    it("removes a person from a team", async () => {
      const { useRemovePersonFromTeam } = await import("./use-admin");
      const { result } = renderHook(() => useRemovePersonFromTeam(), { wrapper: TestWrapper });

      result.current.mutate({ personId: "p-1", teamId: "team-1" });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useListUnassignedPeople", () => {
    it("fetches unassigned people", async () => {
      const { useListUnassignedPeople } = await import("./use-admin");
      const { result } = renderHook(() => useListUnassignedPeople(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toHaveLength(1);
      expect(result.current.data?.[0]?.name).toBe("Unassigned User");
    });
  });

  describe("useAssignGithubTeam", () => {
    it("assigns a GitHub team and succeeds", async () => {
      const { useAssignGithubTeam } = await import("./use-admin");
      const { result } = renderHook(() => useAssignGithubTeam(), { wrapper: TestWrapper });

      result.current.mutate({ teamId: "team-1", githubTeamId: "gh-team-1" });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useUnassignGithubTeam", () => {
    it("unassigns a GitHub team and succeeds", async () => {
      const { useUnassignGithubTeam } = await import("./use-admin");
      const { result } = renderHook(() => useUnassignGithubTeam(), { wrapper: TestWrapper });

      result.current.mutate({ teamId: "team-1", githubTeamId: "gh-team-1" });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useDismissTeamMappingSuggestion", () => {
    it("dismisses a suggestion and succeeds", async () => {
      const { useDismissTeamMappingSuggestion } = await import("./use-admin");
      const { result } = renderHook(() => useDismissTeamMappingSuggestion(), {
        wrapper: TestWrapper,
      });

      result.current.mutate({ teamId: "team-1", githubTeamId: "gh-team-1" });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("adminKeys", () => {
    it("builds hierarchical query keys", async () => {
      const { adminKeys } = await import("./use-admin");
      expect(adminKeys.all).toEqual(["admin"]);
      expect(adminKeys.tokens()).toEqual(["admin", "tokens"]);
    });
  });
});
