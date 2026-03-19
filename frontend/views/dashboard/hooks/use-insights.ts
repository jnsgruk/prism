import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";

import type { OrgInsights } from "@ps/api/gen/prism/v1/insights_pb";
import { InsightsService } from "@ps/api/gen/prism/v1/insights_pb";
import { transport } from "@ps/api/transport";
import { periodKeyToInsightsPeriod } from "@/views/teams/hooks/use-insights";

const insightsClient = createClient(InsightsService, transport);

export const orgInsightsKeys = {
  all: ["insights", "org"] as const,
  org: (period: string, teamId: string) => [...orgInsightsKeys.all, period, teamId] as const,
};

export const useOrgInsights = (
  periodKey: string,
  teamId?: string,
): UseQueryResult<OrgInsights | undefined, Error> => {
  const period = periodKeyToInsightsPeriod(periodKey);
  return useQuery({
    queryKey: orgInsightsKeys.org(period, teamId ?? ""),
    queryFn: () => insightsClient.getOrgInsights({ period, teamId: teamId ?? "" }),
    select: (data) => data.insights,
    staleTime: 5 * 60 * 1000,
  });
};
