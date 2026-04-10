import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { keepPreviousData, useQuery } from "@tanstack/react-query";

import type { GetDiscourseActivityResponse, Period } from "@ps/api/gen/canonical/prism/v1/metrics_pb";
import { MetricsService } from "@ps/api/gen/canonical/prism/v1/metrics_pb";
import { transport } from "@ps/api/transport";

const metricsClient = createClient(MetricsService, transport);

export const useDiscourseActivity = (
  teamId: string,
  period: Period,
  enabled = true,
  instance?: string,
): UseQueryResult<GetDiscourseActivityResponse> =>
  useQuery({
    queryKey: ["discourse-activity", teamId, `${period.type}-${period.start}`, instance ?? "all"],
    queryFn: () =>
      metricsClient.getDiscourseActivity({
        teamId,
        period,
        instance: instance || undefined,
      }),
    enabled: !!teamId && enabled,
    placeholderData: keepPreviousData,
  });
