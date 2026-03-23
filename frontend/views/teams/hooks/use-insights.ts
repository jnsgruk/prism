import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";

import type { TeamInsights } from "@ps/api/gen/canonical/prism/v1/insights_pb";
import { InsightsService } from "@ps/api/gen/canonical/prism/v1/insights_pb";
import { transport } from "@ps/api/transport";

const insightsClient = createClient(InsightsService, transport);

export const insightsKeys = {
  all: ["insights"] as const,
  team: (teamId: string, period: string, includeDescendants: boolean) =>
    [...insightsKeys.all, "team", teamId, period, includeDescendants] as const,
};

/** Map a period selector key ("1w", "1m", etc.) to the insights API period string. */
export const periodKeyToInsightsPeriod = (key: string): string => {
  switch (key) {
    case "1w":
    case "2w":
      return "last_week";
    case "1m":
      return "last_month";
    case "1q":
      return "last_quarter";
    case "1y":
    case "all":
      return "last_year";
    default:
      return "last_month";
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
