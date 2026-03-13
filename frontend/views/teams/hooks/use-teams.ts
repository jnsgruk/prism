import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  CreateTeamResponse,
  DeleteTeamResponse,
  GetTeamResponse,
  GetTeamTreeResponse,
  ImportDirectoryResponse,
  Person,
  Team,
  UpdateTeamResponse,
} from "@ps/api/gen/prism/v1/org_pb";
import { OrgService, TeamType } from "@ps/api/gen/prism/v1/org_pb";
import { transport } from "@ps/api/transport";

const orgClient = createClient(OrgService, transport);

export const orgKeys = {
  all: ["org"] as const,
  teams: (parentTeamId?: string): readonly ["org", "teams", string | undefined] =>
    [...orgKeys.all, "teams", parentTeamId] as const,
  tree: (): readonly ["org", "tree"] => [...orgKeys.all, "tree"] as const,
  team: (teamId: string): readonly ["org", "team", string] =>
    [...orgKeys.all, "team", teamId] as const,
  people: (): readonly ["org", "people"] => [...orgKeys.all, "people"] as const,
};

export const useListTeams = (parentTeamId?: string): UseQueryResult<Team[], Error> =>
  useQuery({
    queryKey: orgKeys.teams(parentTeamId),
    queryFn: () => orgClient.listTeams({ parentTeamId }),
    select: (data): Team[] => data.teams,
  });

export const useGetTeamTree = (): UseQueryResult<GetTeamTreeResponse, Error> =>
  useQuery({
    queryKey: orgKeys.tree(),
    queryFn: () => orgClient.getTeamTree({}),
  });

export const useGetTeam = (teamId: string): UseQueryResult<GetTeamResponse, Error> =>
  useQuery({
    queryKey: orgKeys.team(teamId),
    queryFn: () => orgClient.getTeam({ teamId }),
    enabled: !!teamId,
  });

export const useListPeople = (): UseQueryResult<Person[], Error> =>
  useQuery({
    queryKey: orgKeys.people(),
    queryFn: () => orgClient.listPeople({}),
    select: (data): Person[] => data.people,
  });

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

export const useCreateTeam = (): UseMutationResult<
  CreateTeamResponse,
  Error,
  {
    name: string;
    teamType: TeamType;
    orgName: string;
    parentTeamId?: string;
    leadId?: string;
    githubTeamSlug?: string;
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
  { teamId: string; name?: string; parentTeamId?: string; leadId?: string; githubTeamSlug?: string }
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

/** Human-readable label for a TeamType enum value. */
export const teamTypeLabel = (t: TeamType): string => {
  switch (t) {
    case TeamType.ORG:
      return "Org";
    case TeamType.GROUP:
      return "Group";
    case TeamType.TEAM:
      return "Team";
    case TeamType.SQUAD:
      return "Squad";
    default:
      return "Unknown";
  }
};

/** Badge variant for a team type. */
export const teamTypeBadgeVariant = (
  t: TeamType,
): "default" | "secondary" | "outline" | "destructive" => {
  switch (t) {
    case TeamType.ORG:
      return "default";
    case TeamType.GROUP:
      return "secondary";
    case TeamType.TEAM:
      return "outline";
    case TeamType.SQUAD:
      return "outline";
    default:
      return "secondary";
  }
};
