import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  GetTeamResponse,
  ImportDirectoryResponse,
  Person,
  Team,
} from "@ps/api/gen/prism/v1/org_pb";
import { OrgService } from "@ps/api/gen/prism/v1/org_pb";
import { transport } from "@ps/api/transport";

const orgClient = createClient(OrgService, transport);

export const orgKeys = {
  all: ["org"] as const,
  teams: (parentTeamId?: string): readonly ["org", "teams", string | undefined] =>
    [...orgKeys.all, "teams", parentTeamId] as const,
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
