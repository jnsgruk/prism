import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";

import type {
  Contribution,
  ListTeamContributionsResponse,
  Period,
  TeamMetrics,
} from "@ps/api/gen/prism/v1/metrics_pb";
import { MetricsService, PeriodType } from "@ps/api/gen/prism/v1/metrics_pb";
import { transport } from "@ps/api/transport";

const metricsClient = createClient(MetricsService, transport);

export const metricsKeys = {
  all: ["metrics"] as const,
  compare: (teamIds: string[], period: Period) =>
    [...metricsKeys.all, "compare", ...teamIds, `${period.type}-${period.start}`] as const,
  periods: () => [...metricsKeys.all, "periods"] as const,
  contributions: (teamId: string, period: Period, filters: ContributionFilters) =>
    [
      ...metricsKeys.all,
      "contributions",
      teamId,
      `${period.type}-${period.start}`,
      filters.contributionType ?? "",
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
      }),
    enabled: teamId.length > 0,
  });

export type { Contribution };
export { PeriodType };
