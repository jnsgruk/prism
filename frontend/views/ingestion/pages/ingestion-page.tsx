import { PageHeader } from "@/components/page-header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Activity, Database, Loader2 } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { toast } from "sonner";

import { SourceState } from "@ps/api/gen/canonical/prism/v1/handlers_pb";

import { AiPipelineStatus } from "@/views/ingestion/components/ai-pipeline-status";
import { RunHistoryPanel } from "@/views/ingestion/components/ingestion-runs-table";
import { IngestionActions, IngestionSummary } from "@/views/ingestion/components/ingestion-summary";
import { PipelineGraph } from "@/views/ingestion/components/pipeline-graph";
import { SourceList } from "@/views/ingestion/components/source-list";
import {
  useIngestionStatus,
  useListRuns,
  useTriggerRun,
} from "@/views/ingestion/hooks/use-ingestion";

const POLL_INTERVAL_BURST = 1_000;
const POLL_INTERVAL_ACTIVE = 2_000;
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

  const triggerRun = useTriggerRun();

  const handleRunAll = useCallback(() => {
    if (!sources) return;
    const idle = sources.filter(
      (s) => s.state !== SourceState.COLLECTING && s.state !== SourceState.WAITING,
    );
    if (idle.length === 0) {
      toast.info("All sources are already running");
      return;
    }
    for (const s of idle) {
      triggerRun.mutate(s.name);
    }
    toast.success(`Triggered ${idle.length} source${idle.length > 1 ? "s" : ""}`);
    triggerBurst();
  }, [sources, triggerRun, triggerBurst]);

  const hasActiveRun = sources?.some((s) => s.state === SourceState.COLLECTING);

  let runsInterval = POLL_INTERVAL_IDLE;
  if (isBursting) runsInterval = POLL_INTERVAL_BURST;
  else if (hasActiveRun) runsInterval = POLL_INTERVAL_ACTIVE;

  const { data: runs, isLoading: runsLoading } = useListRuns(undefined, {
    refetchInterval: runsInterval,
    ingestionOnly: true,
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
        <PipelineGraph onAction={triggerBurst} />

        <Card>
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3">
                <CardTitle className="flex items-center gap-2 text-sm font-semibold">
                  <Database className="size-4" />
                  Ingestion Pipeline
                </CardTitle>
                <IngestionSummary sources={sources} />
              </div>
              <IngestionActions
                sources={sources}
                onRunAll={handleRunAll}
                isPending={triggerRun.isPending}
              />
            </div>
          </CardHeader>
          <CardContent className="px-0 pb-0">
            <SourceList sources={sources} onAction={triggerBurst} />
          </CardContent>
        </Card>

        <AiPipelineStatus />

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
