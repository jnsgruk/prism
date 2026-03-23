import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";

import { InsightPeriod } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { TeamInsights } from "@ps/api/gen/canonical/prism/v1/insights_pb";
import { InsightsService } from "@ps/api/gen/canonical/prism/v1/insights_pb";
import { transport } from "@ps/api/transport";

const insightsClient = createClient(InsightsService, transport);

export const insightsKeys = {
  all: ["insights"] as const,
  team: (teamId: string, period: InsightPeriod, includeDescendants: boolean) =>
    [...insightsKeys.all, "team", teamId, period, includeDescendants] as const,
};

/** Map a period selector key ("1w", "1m", etc.) to the InsightPeriod enum. */
export const periodKeyToInsightsPeriod = (key: string): InsightPeriod => {
  switch (key) {
    case "1w":
    case "2w":
      return InsightPeriod.LAST_WEEK;
    case "1m":
      return InsightPeriod.LAST_MONTH;
    case "1q":
      return InsightPeriod.LAST_QUARTER;
    case "1y":
    case "all":
      return InsightPeriod.LAST_YEAR;
    default:
      return InsightPeriod.LAST_MONTH;
  }
};

export const useTeamInsights = (
  teamId: string,
  periodKey: string,
  includeDescendants: boolean,
): UseQueryResult<TeamInsights | undefined, Error> => {
  const period = periodKeyToInsightsPeriod(periodKey);
  return useQuery({
    queryKey: insightsKeys.team(teamId, period, includeDescendants),
    queryFn: () => insightsClient.getTeamInsights({ teamId, period, includeDescendants }),
    select: (data) => data.insights,
    enabled: teamId.length > 0,
    staleTime: 5 * 60 * 1000,
  });
};
