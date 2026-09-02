import { POLL_INTERVAL_ACTIVE, POLL_INTERVAL_IDLE } from "@/views/ingestion/lib/constants";

import { RunStatus } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { HandlerRun } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { useListRuns } from "@ps/hooks/use-ingestion";

export const useTeamSyncStatus = (sourceName: string): { isRunning: boolean; latestRun: HandlerRun | undefined } => {
  const { data: runs } = useListRuns(sourceName, {
    handlerName: "GithubTeamSyncHandler",
    refetchInterval: (query) =>
      query.state.data?.runs[0]?.status === RunStatus.RUNNING ? POLL_INTERVAL_ACTIVE : POLL_INTERVAL_IDLE,
  });

  const latestRun = runs?.[0];
  const running = latestRun?.status === RunStatus.RUNNING;

  return { isRunning: running, latestRun };
};
