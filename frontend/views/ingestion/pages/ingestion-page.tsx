import { PageHeader } from "@/components/page-header";
import { Activity, Loader2 } from "lucide-react";
import { useCallback, useRef, useState } from "react";

import { SourceState } from "@ps/api/gen/prism/v1/handlers_pb";

import { RunHistoryPanel } from "@/views/ingestion/components/ingestion-runs-table";
import { SourceStatusRow } from "@/views/ingestion/components/source-status-card";
import { useIngestionStatus, useListRuns } from "@/views/ingestion/hooks/use-ingestion";

const POLL_INTERVAL_BURST = 1_000;
const POLL_INTERVAL_ACTIVE = 3_000;
const POLL_INTERVAL_IDLE = 30_000;
const BURST_DURATION = 10_000;

const IngestionPage = (): React.ReactElement => {
  const [burstUntil, setBurstUntil] = useState(0);
  const burstTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const triggerBurst = useCallback(() => {
    setBurstUntil(Date.now() + BURST_DURATION);
    clearTimeout(burstTimerRef.current);
    burstTimerRef.current = setTimeout(() => setBurstUntil(0), BURST_DURATION);
  }, []);

  const isBursting = burstUntil > Date.now();

  const { data: sources, isLoading: sourcesLoading } = useIngestionStatus({
    refetchInterval: (query) => {
      if (isBursting) return POLL_INTERVAL_BURST;
      const data = query.state.data?.sources;
      const hasActive = data?.some(
        (s) => s.state === SourceState.COLLECTING || s.state === SourceState.WAITING,
      );
      return hasActive ? POLL_INTERVAL_ACTIVE : POLL_INTERVAL_IDLE;
    },
  });

  const hasActiveRun = sources?.some((s) => s.state === SourceState.COLLECTING);

  let runsInterval = POLL_INTERVAL_IDLE;
  if (isBursting) runsInterval = POLL_INTERVAL_BURST;
  else if (hasActiveRun) runsInterval = POLL_INTERVAL_ACTIVE;

  const { data: runs, isLoading: runsLoading } = useListRuns(undefined, {
    refetchInterval: runsInterval,
  });

  if (sourcesLoading) {
    return (
      <>
        <PageHeader title="Ingestion" description="Monitor data source ingestion runs" />
        <div className="flex flex-1 items-center justify-center p-6">
          <Loader2 className="size-6 animate-spin text-muted-foreground" />
        </div>
      </>
    );
  }

  if (!sources || sources.length === 0) {
    return (
      <>
        <PageHeader title="Ingestion" description="Monitor data source ingestion runs" />
        <div className="flex-1 p-6">
          <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
            <Activity className="mb-3 size-10 text-muted-foreground" />
            <p className="mb-1 font-medium">No sources configured</p>
            <p className="text-sm text-muted-foreground">
              Add a data source in the Admin page to start ingesting data.
            </p>
          </div>
        </div>
      </>
    );
  }

  const sourceNames = sources.map((s) => s.name);

  return (
    <>
      <PageHeader title="Ingestion" description="Monitor data source ingestion runs" />
      <div className="flex-1 space-y-6 p-6">
        <section>
          <h2 className="mb-3 text-sm font-semibold">Sources</h2>
          <div className="space-y-3">
            {sources.map((source) => (
              <SourceStatusRow key={source.name} source={source} onAction={triggerBurst} />
            ))}
          </div>
        </section>

        {runsLoading ? (
          <div className="flex justify-center py-8">
            <Loader2 className="size-5 animate-spin text-muted-foreground" />
          </div>
        ) : (
          <RunHistoryPanel runs={runs ?? []} sourceNames={sourceNames} />
        )}
      </div>
    </>
  );
};

export default IngestionPage;
