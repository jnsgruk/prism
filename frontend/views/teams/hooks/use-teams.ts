import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { keepPreviousData, useQuery } from "@tanstack/react-query";

import type {
  GetTeamResponse,
  GetTeamTreeResponse,
  ListPeopleResponse,
  Person,
  Team,
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

export interface PeopleQueryParams {
  search?: string;
  filter?: string;
  pageSize: number;
  pageToken?: string;
  sortField?: string;
  sortDesc?: boolean;
}

export const usePaginatedPeople = (
  params: PeopleQueryParams,
): UseQueryResult<ListPeopleResponse, Error> =>
  useQuery({
    queryKey: [...orgKeys.people(), params] as const,
    queryFn: () =>
      orgClient.listPeople({
        search: params.search || undefined,
        filter: params.filter || undefined,
        pagination: { pageSize: params.pageSize, pageToken: params.pageToken ?? "" },
        sort: params.sortField
          ? { field: params.sortField, descending: params.sortDesc ?? false }
          : undefined,
      }),
    placeholderData: keepPreviousData,
  });

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
