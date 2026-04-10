import { periodKeyToInsightsPeriod } from "@/views/teams/hooks/use-insights";
import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";

import type { InsightPeriod } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { PersonInsights } from "@ps/api/gen/canonical/prism/v1/insights_pb";
import { InsightsService } from "@ps/api/gen/canonical/prism/v1/insights_pb";
import { transport } from "@ps/api/transport";

const insightsClient = createClient(InsightsService, transport);

export const personInsightsKeys = {
  all: ["insights", "person"] as const,
  person: (personId: string, period: InsightPeriod) => [...personInsightsKeys.all, personId, period] as const,
};

export const usePersonInsights = (personId: string, periodKey: string): UseQueryResult<PersonInsights | undefined> => {
  const period = periodKeyToInsightsPeriod(periodKey);
  return useQuery({
    queryKey: personInsightsKeys.person(personId, period),
    queryFn: () => insightsClient.getPersonInsights({ personId, period }),
    select: (data) => data.insights,
    enabled: personId.length > 0,
    staleTime: 5 * 60 * 1000,
  });
};
