import { createClient } from "@connectrpc/connect";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { OrgService } from "@ps/api/gen/prism/v1/org_pb";
import { transport } from "@ps/api/transport";

const orgClient = createClient(OrgService, transport);

export const orgKeys = {
  all: ["org"] as const,
  teams: (parentTeamId?: string) => [...orgKeys.all, "teams", parentTeamId] as const,
  team: (teamId: string) => [...orgKeys.all, "team", teamId] as const,
  people: () => [...orgKeys.all, "people"] as const,
};

export const useListTeams = (parentTeamId?: string) =>
  useQuery({
    queryKey: orgKeys.teams(parentTeamId),
    queryFn: () => orgClient.listTeams({ parentTeamId }),
    select: (data) => data.teams,
  });

export const useGetTeam = (teamId: string) =>
  useQuery({
    queryKey: orgKeys.team(teamId),
    queryFn: () => orgClient.getTeam({ teamId }),
    enabled: !!teamId,
  });

export const useListPeople = () =>
  useQuery({
    queryKey: orgKeys.people(),
    queryFn: () => orgClient.listPeople({}),
    select: (data) => data.people,
  });

export const useImportDirectory = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (fileContent: Uint8Array) => orgClient.importDirectory({ fileContent }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
    },
  });
};
