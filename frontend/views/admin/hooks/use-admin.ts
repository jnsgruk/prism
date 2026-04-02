import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  AdminService,
  type ApiTokenInfo,
  type CreateApiTokenResponse,
  type ResetDataResponse,
} from "@ps/api/gen/canonical/prism/v1/admin_pb";
import type {
  CreateTeamResponse,
  DeleteTeamResponse,
  ImportDirectoryResponse,
  ImportJiraUsersResponse,
  Person,
  UpdatePersonResponse,
  UpdateTeamResponse,
} from "@ps/api/gen/canonical/prism/v1/org_pb";
import { OrgService, TeamType } from "@ps/api/gen/canonical/prism/v1/org_pb";
import { transport } from "@ps/api/transport";
import { configKeys } from "@ps/hooks/use-config";

import { orgKeys } from "@/lib/hooks/use-org";

const adminClient = createClient(AdminService, transport);
const orgClient = createClient(OrgService, transport);

export const adminKeys = {
  all: ["admin"] as const,
  tokens: (): readonly ["admin", "tokens"] => [...adminKeys.all, "tokens"] as const,
};

export const useListApiTokens = (): UseQueryResult<ApiTokenInfo[], Error> =>
  useQuery({
    queryKey: adminKeys.tokens(),
    queryFn: () => adminClient.listApiTokens({}),
    select: (data): ApiTokenInfo[] => data.tokens,
  });

export const useCreateApiToken = (): UseMutationResult<CreateApiTokenResponse, Error, string> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (name: string) => adminClient.createApiToken({ name }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.tokens() });
    },
  });
};

export const useRevokeApiToken = (): UseMutationResult<void, Error, string> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (tokenId: string) => {
      await adminClient.revokeApiToken({ tokenId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: adminKeys.tokens() });
    },
  });
};

export const useResetData = (): UseMutationResult<ResetDataResponse, Error, void> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => adminClient.resetData({ confirm: true }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: configKeys.sources() });
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
      queryClient.invalidateQueries({ queryKey: ["metrics"] });
    },
  });
};

export const useImportDirectory = (): UseMutationResult<
  ImportDirectoryResponse,
  Error,
  Uint8Array
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (fileContent: Uint8Array) => orgClient.importDirectory({ fileContent }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useImportJiraUsers = (): UseMutationResult<
  ImportJiraUsersResponse,
  Error,
  { fileContent: Uint8Array; sourceName: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: ({ fileContent, sourceName }) =>
      orgClient.importJiraUsers({ fileContent, sourceName }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useCreateTeam = (): UseMutationResult<
  CreateTeamResponse,
  Error,
  {
    name: string;
    teamType: TeamType;
    orgName: string;
    parentTeamId?: string;
    leadId?: string;
  }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input) => orgClient.createTeam(input),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useUpdateTeam = (): UseMutationResult<
  UpdateTeamResponse,
  Error,
  { teamId: string; name?: string; parentTeamId?: string; leadId?: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input) => orgClient.updateTeam(input),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useDeleteTeam = (): UseMutationResult<DeleteTeamResponse, Error, string> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (teamId: string) => orgClient.deleteTeam({ teamId }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useUpdatePerson = (): UseMutationResult<
  UpdatePersonResponse,
  Error,
  { personId: string; name?: string; email?: string; level?: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (input) => orgClient.updatePerson(input),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useDeactivatePerson = (): UseMutationResult<void, Error, string> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (personId: string) => {
      await orgClient.deactivatePerson({ personId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useReactivatePerson = (): UseMutationResult<void, Error, string> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (personId: string) => {
      await orgClient.reactivatePerson({ personId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useAssignPersonToTeam = (): UseMutationResult<
  void,
  Error,
  { personId: string; teamId: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ personId, teamId }) => {
      await orgClient.assignPersonToTeam({ personId, teamId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useRemovePersonFromTeam = (): UseMutationResult<
  void,
  Error,
  { personId: string; teamId: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ personId, teamId }) => {
      await orgClient.removePersonFromTeam({ personId, teamId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useListUnassignedPeople = (): UseQueryResult<Person[], Error> =>
  useQuery({
    queryKey: [...orgKeys.all, "unassigned"] as const,
    queryFn: () => orgClient.listUnassignedPeople({}),
    select: (data): Person[] => data.people,
  });

export const useAssignGithubTeam = (): UseMutationResult<
  void,
  Error,
  { teamId: string; githubTeamId: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ teamId, githubTeamId }) => {
      await orgClient.assignGithubTeam({ teamId, githubTeamId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useUnassignGithubTeam = (): UseMutationResult<
  void,
  Error,
  { teamId: string; githubTeamId: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ teamId, githubTeamId }) => {
      await orgClient.unassignGithubTeam({ teamId, githubTeamId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};

export const useDismissTeamMappingSuggestion = (): UseMutationResult<
  void,
  Error,
  { teamId: string; githubTeamId: string }
> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ teamId, githubTeamId }) => {
      await orgClient.dismissTeamMappingSuggestion({ teamId, githubTeamId });
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};
