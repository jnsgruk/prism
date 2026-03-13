import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";

import type { Period, TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";
import { MetricsService, PeriodType } from "@ps/api/gen/prism/v1/metrics_pb";
import { transport } from "@ps/api/transport";

const metricsClient = createClient(MetricsService, transport);

export const metricsKeys = {
  all: ["metrics"] as const,
  compare: (teamIds: string[], period: Period): readonly string[] => [
    ...metricsKeys.all,
    "compare",
    ...teamIds,
    `${period.type}-${period.start}`,
  ],
  periods: (): readonly string[] => [...metricsKeys.all, "periods"],
};

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

export { PeriodType };
