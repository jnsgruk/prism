import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";

import type {
  Contribution,
  GetFlowMetricsResponse,
  GetIndividualProfileResponse,
  ListPersonContributionsResponse,
  ListTeamContributionsResponse,
  Period,
  TeamMetrics,
} from "@ps/api/gen/canonical/prism/v1/metrics_pb";
import { MetricsService, PeriodType } from "@ps/api/gen/canonical/prism/v1/metrics_pb";
import { transport } from "@ps/api/transport";

const metricsClient = createClient(MetricsService, transport);

const periodKey = (period: Period): string => `${period.type}-${period.start}`;

export const metricsKeys = {
  all: ["metrics"] as const,
  compare: (teamIds: string[], period: Period) =>
    [...metricsKeys.all, "compare", ...teamIds, periodKey(period)] as const,
  periods: () => [...metricsKeys.all, "periods"] as const,
  contributions: (teamId: string, period: Period, filters: ContributionFilters) =>
    [
      ...metricsKeys.all,
      "contributions",
      teamId,
      periodKey(period),
      filters.contributionType ?? "",
      filters.state ?? "",
      filters.search ?? "",
      filters.sortField ?? "",
      filters.sortDesc ?? true,
      filters.pageSize,
      filters.pageIndex,
      filters.platform ?? "",
    ] as const,
  flow: (teamId: string, period: Period) =>
    [...metricsKeys.all, "flow", teamId, periodKey(period)] as const,
  individual: (personId: string, period: Period) =>
    [...metricsKeys.all, "individual", personId, periodKey(period)] as const,
  personContributions: (personId: string, filters: PersonContributionFilters) =>
    [
      ...metricsKeys.all,
      "person-contributions",
      personId,
      filters.platform ?? "",
      filters.contributionType ?? "",
      filters.since ?? "",
      filters.state ?? "",
      filters.search ?? "",
      filters.sortField ?? "",
      filters.sortDesc ?? true,
      filters.pageSize,
      filters.pageIndex,
    ] as const,
};

export interface ContributionFilters {
  contributionType?: string;
  state?: string;
  search?: string;
  sortField?: string;
  sortDesc?: boolean;
  pageSize: number;
  pageIndex: number;
  platform?: string;
}

export const useCompareTeams = (
  teamIds: string[],
  period: Period,
): UseQueryResult<TeamMetrics[], Error> =>
  useQuery({
    queryKey: metricsKeys.compare(teamIds, period),
    queryFn: () => metricsClient.compareTeams({ teamIds, period }),
    select: (data): TeamMetrics[] => data.metrics,
    enabled: teamIds.length > 0,
  });

export const useListPeriods = (): UseQueryResult<Period[], Error> =>
  useQuery({
    queryKey: metricsKeys.periods(),
    queryFn: () => metricsClient.listPeriods({}),
    select: (data): Period[] => data.periods,
  });

export const useListTeamContributions = (
  teamId: string,
  period: Period,
  filters: ContributionFilters,
): UseQueryResult<ListTeamContributionsResponse, Error> =>
  useQuery({
    queryKey: metricsKeys.contributions(teamId, period, filters),
    queryFn: () =>
      metricsClient.listTeamContributions({
        teamId,
        period,
        contributionType: filters.contributionType,
        state: filters.state,
        search: filters.search,
        sortField: filters.sortField,
        sortDesc: filters.sortDesc,
        pageSize: filters.pageSize,
        pageIndex: filters.pageIndex,
        platform: filters.platform,
      }),
    enabled: teamId.length > 0,
  });

export const useGetFlowMetrics = (
  teamId: string,
  period: Period,
): UseQueryResult<GetFlowMetricsResponse, Error> =>
  useQuery({
    queryKey: metricsKeys.flow(teamId, period),
    queryFn: () => metricsClient.getFlowMetrics({ teamId, period }),
    enabled: teamId.length > 0,
  });

export interface PersonContributionFilters {
  platform?: string;
  contributionType?: string;
  since?: string;
  sortField?: string;
  sortDesc?: boolean;
  pageSize: number;
  pageIndex: number;
  state?: string;
  search?: string;
}

export const useGetIndividualProfile = (
  personId: string,
  period: Period,
): UseQueryResult<GetIndividualProfileResponse, Error> =>
  useQuery({
    queryKey: metricsKeys.individual(personId, period),
    queryFn: () => metricsClient.getIndividualProfile({ personId, period }),
    enabled: personId.length > 0,
  });

export const useListPersonContributions = (
  personId: string,
  filters: PersonContributionFilters,
): UseQueryResult<ListPersonContributionsResponse, Error> =>
  useQuery({
    queryKey: metricsKeys.personContributions(personId, filters),
    queryFn: () =>
      metricsClient.listPersonContributions({
        personId,
        platform: filters.platform,
        contributionType: filters.contributionType,
        since: filters.since,
        state: filters.state,
        search: filters.search,
        sortField: filters.sortField,
        sortDesc: filters.sortDesc,
        pageSize: filters.pageSize,
        pageIndex: filters.pageIndex,
      }),
    enabled: personId.length > 0,
  });

export const useContribution = (contributionId: string): UseQueryResult<Contribution, Error> =>
  useQuery({
    queryKey: [...metricsKeys.all, "contribution", contributionId] as const,
    queryFn: () => metricsClient.getContribution({ contributionId }),
    select: (data): Contribution => data.contribution!,
    enabled: contributionId.length > 0,
  });

export type { Contribution, GetFlowMetricsResponse, GetIndividualProfileResponse };
export { PeriodType };
