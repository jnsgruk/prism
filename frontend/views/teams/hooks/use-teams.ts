import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { keepPreviousData, useQuery } from "@tanstack/react-query";

import type { PersonFilter } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type {
  GetTeamResponse,
  GetTeamTreeResponse,
  GitHubTeam,
  ListPeopleResponse,
  Person,
  Team,
  TeamMappingSuggestion,
} from "@ps/api/gen/canonical/prism/v1/org_pb";
import { OrgService, TeamType } from "@ps/api/gen/canonical/prism/v1/org_pb";
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
  githubTeams: (search?: string, githubOrg?: string) =>
    [...orgKeys.all, "github-teams", search, githubOrg] as const,
  teamGithubTeams: (teamId: string) => [...orgKeys.all, "team-github-teams", teamId] as const,
  mappingSuggestions: () => [...orgKeys.all, "mapping-suggestions"] as const,
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
  filter?: PersonFilter;
  teamId?: string;
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
        teamId: params.teamId || undefined,
        pagination: { pageSize: params.pageSize, pageToken: params.pageToken ?? "" },
        sort: params.sortField
          ? { field: params.sortField, descending: params.sortDesc ?? false }
          : undefined,
      }),
    placeholderData: keepPreviousData,
  });

/** Flatten a team tree into a list with depth info. */
export interface FlatTeam {
  team: Team;
  depth: number;
}

export const flattenTree = (roots: Team[], depth = 0): FlatTeam[] =>
  roots.flatMap((team) => [{ team, depth }, ...flattenTree(team.children, depth + 1)]);

/** Find a team by ID in a tree. */
export const findTeam = (roots: Team[], id: string): Team | undefined => {
  for (const root of roots) {
    if (root.id === id) return root;
    const found = findTeam(root.children, id);
    if (found) return found;
  }
  return undefined;
};

/** Get ancestor chain from root to the team with the given ID (inclusive). */
export const getAncestors = (roots: Team[], id: string): Team[] => {
  for (const root of roots) {
    if (root.id === id) return [root];
    const path = getAncestors(root.children, id);
    if (path.length > 0) return [root, ...path];
  }
  return [];
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

export const useListGithubTeams = (
  search?: string,
  githubOrg?: string,
): UseQueryResult<GitHubTeam[], Error> =>
  useQuery({
    queryKey: orgKeys.githubTeams(search, githubOrg),
    queryFn: () =>
      orgClient.listGithubTeams({
        search: search || undefined,
        githubOrg: githubOrg || undefined,
      }),
    select: (data): GitHubTeam[] => data.teams,
  });

export const useListTeamGithubTeams = (teamId: string): UseQueryResult<GitHubTeam[], Error> =>
  useQuery({
    queryKey: orgKeys.teamGithubTeams(teamId),
    queryFn: () => orgClient.listTeamGithubTeams({ teamId }),
    select: (data): GitHubTeam[] => data.teams,
    enabled: !!teamId,
  });

export const useGetTeamMappingSuggestions = (): UseQueryResult<TeamMappingSuggestion[], Error> =>
  useQuery({
    queryKey: orgKeys.mappingSuggestions(),
    queryFn: () => orgClient.getTeamMappingSuggestions({}),
    select: (data): TeamMappingSuggestion[] => data.suggestions,
  });
