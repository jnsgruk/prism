import { aiKeys } from "@/lib/hooks/use-ai-settings";
import { createClient } from "@connectrpc/connect";
import type { UseQueryResult } from "@tanstack/react-query";
import { useQuery } from "@tanstack/react-query";

import type { GetUsageSummaryResponse } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { ReasoningService } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { transport } from "@ps/api/transport";

const client = createClient(ReasoningService, transport);

export const useUsageSummary = (days = 7): UseQueryResult<GetUsageSummaryResponse, Error> =>
  useQuery({
    queryKey: aiKeys.usage(days),
    queryFn: () => client.getUsageSummary({ days }),
    refetchInterval: 60_000,
  });
