import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";

import type { GetDiscourseActivityResponse, Period } from "@ps/api/gen/prism/v1/metrics_pb";
import { MetricsService } from "@ps/api/gen/prism/v1/metrics_pb";
import { transport } from "@ps/api/transport";

const metricsClient = createClient(MetricsService, transport);

export const useDiscourseActivity = (
  teamId: string,
  period: Period,
): UseQueryResult<GetDiscourseActivityResponse, Error> =>
  useQuery({
    queryKey: ["discourse-activity", teamId, `${period.type}-${period.start}`],
    queryFn: () => metricsClient.getDiscourseActivity({ teamId, period }),
    enabled: !!teamId,
  });
