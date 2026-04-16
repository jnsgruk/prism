import { POLL_INTERVAL_ACTIVE, POLL_INTERVAL_IDLE } from "@/views/ingestion/lib/constants";
import { useEffect, useState } from "react";

import { RunStatus } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { HandlerRun } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { useListRuns } from "@ps/hooks/use-ingestion";

export const useTeamSyncStatus = (sourceName: string): { isRunning: boolean; latestRun: HandlerRun | undefined } => {
  const [isRunning, setIsRunning] = useState(false);

  const { data: runs } = useListRuns(sourceName, {
    handlerName: "GithubTeamSyncHandler",
    refetchInterval: isRunning ? POLL_INTERVAL_ACTIVE : POLL_INTERVAL_IDLE,
  });

  const latestRun = runs?.[0];
  const running = latestRun?.status === RunStatus.RUNNING;

  useEffect(() => {
    setIsRunning(running);
  }, [running]);

  return { isRunning: running, latestRun };
};
