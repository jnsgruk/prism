import { PageHeader } from "@/components/page-header";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { formatRelativeTime } from "@/lib/format";
import { useListSources, useUpdateSource } from "@/lib/hooks/use-config";
import { IngestionSummary } from "@/views/ingestion/components/ingestion-summary";
import { PipelineActions } from "@/views/ingestion/components/pipeline-actions";
import { PipelineDAG, StatusBadge, usePipelineState } from "@/views/ingestion/components/pipeline-graph";
import { PipelineRunHistoryPanel } from "@/views/ingestion/components/pipeline-run-history";
import { SourceList, type SourceConfigInfo } from "@/views/ingestion/components/source-list";
import { useIngestionStatus } from "@/views/ingestion/hooks/use-ingestion";
import { POLL_INTERVAL_ACTIVE, POLL_INTERVAL_BURST, POLL_INTERVAL_IDLE } from "@/views/ingestion/lib/constants";
import { Activity, ChevronRight, GitBranch, Loader2 } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";
import { toast } from "sonner";

import { SourceState } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { cn } from "@ps/cn";

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

  const { current: currentPipeline, isRunning: pipelineRunning } = usePipelineState({
    isBursting,
  });

  // DAG auto-expands when running, collapsed when idle
  const [dagOpen, setDagOpen] = useState(false);
  const dagExpanded = pipelineRunning || dagOpen;

  const { data: sources, isLoading: sourcesLoading } = useIngestionStatus({
    refetchInterval: (query) => {
      if (isBursting) return POLL_INTERVAL_BURST;
      const data = query.state.data?.sources;
      const hasActive = data?.some((s) => s.state === SourceState.COLLECTING);
      return hasActive || pipelineRunning ? POLL_INTERVAL_ACTIVE : POLL_INTERVAL_IDLE;
    },
  });

  // Fetch source configs for enabled/disabled state
  const { data: sourceConfigs } = useListSources();
  const updateSource = useUpdateSource();

  const sourceConfigMap = useMemo(() => {
    if (!sourceConfigs) return undefined;
    const map = new Map<string, SourceConfigInfo>();
    for (const cfg of sourceConfigs) {
      map.set(cfg.name, { id: cfg.id, enabled: cfg.enabled });
    }
    return map;
  }, [sourceConfigs]);

  const disabledCount = useMemo(() => {
    if (!sourceConfigMap) return 0;
    let count = 0;
    for (const v of sourceConfigMap.values()) {
      if (!v.enabled) count++;
    }
    return count;
  }, [sourceConfigMap]);

  const handleToggleEnabled = useCallback(
    (sourceId: string, enabled: boolean) => {
      updateSource.mutate(
        { sourceId, enabled },
        {
          onSuccess: () => toast.success(enabled ? "Source enabled" : "Source disabled"),
          onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to update source"),
        },
      );
    },
    [updateSource],
  );

  const hasActiveRun = sources?.some((s) => s.state === SourceState.COLLECTING);

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

  return (
    <>
      <PageHeader title="Ingestion" description="Monitor data source ingestion runs" />
      <div className="flex-1 space-y-6 p-6">
        {/* Unified Pipeline Card */}
        <Card className="pb-0">
          <Collapsible open={dagExpanded} onOpenChange={setDagOpen}>
            <CardHeader className="pb-3">
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-3">
                  <CardTitle className="flex items-center gap-2 text-sm font-semibold">
                    <GitBranch className="size-4" />
                    Pipeline
                  </CardTitle>
                  {currentPipeline && <StatusBadge status={currentPipeline.status} />}
                  {currentPipeline?.startedAt && (
                    <span className="text-xs text-muted-foreground">
                      {formatRelativeTime(currentPipeline.startedAt)}
                    </span>
                  )}
                  <IngestionSummary sources={sources} disabledCount={disabledCount} />
                  {/* DAG toggle — only when not running (auto-expanded when running) */}
                  {!pipelineRunning && currentPipeline && (
                    <CollapsibleTrigger className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground">
                      <ChevronRight className={cn("size-3.5 transition-transform", dagExpanded && "rotate-90")} />
                      <span className="hidden sm:inline">{dagExpanded ? "Hide" : "Show"}</span>
                    </CollapsibleTrigger>
                  )}
                </div>
                <PipelineActions pipelineId={currentPipeline?.id} isRunning={pipelineRunning} onAction={triggerBurst} />
              </div>
            </CardHeader>

            {/* Collapsible DAG */}
            <CollapsibleContent>
              <CardContent className="pb-4">
                <PipelineDAG sources={sources} isBursting={isBursting} />
              </CardContent>
            </CollapsibleContent>
          </Collapsible>

          {/* Source list + AI handlers */}
          <CardContent className="px-0 pb-0">
            <SourceList sources={sources} sourceConfigs={sourceConfigMap} onToggleEnabled={handleToggleEnabled} />
          </CardContent>
        </Card>

        {/* Run History */}
        <PipelineRunHistoryPanel hasActiveRun={hasActiveRun || pipelineRunning} />
      </div>
    </>
  );
};

export default IngestionPage;
