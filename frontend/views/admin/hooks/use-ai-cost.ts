import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";

import type { GetCostSummaryResponse } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { ReasoningService } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

import { aiKeys } from "@/lib/hooks/use-ai-settings";

const client = createClient(ReasoningService, transport);

export const useCostSummary = (days = 7): UseQueryResult<GetCostSummaryResponse, Error> =>
  useQuery({
    queryKey: aiKeys.cost(days),
    queryFn: () => client.getCostSummary({ days }),
    refetchInterval: 60_000,
  });
