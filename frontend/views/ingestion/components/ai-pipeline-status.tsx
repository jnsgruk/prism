import { DOT_SEP, Stat } from "@/components/inline-stat";
import { CancelButton, RunButton } from "@/components/run-cancel-buttons";
import { StatusDot } from "@/components/status-dot";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Brain } from "lucide-react";
import { useCallback } from "react";
import { toast } from "sonner";

import { formatRelativeTime } from "@/lib/format";
import { useEmbeddingStatus } from "@/lib/hooks/use-embeddings";
import { useEnrichmentPipelineStatus } from "@/views/admin/hooks/use-enrichment";
import {
  useCancelHandlerRun,
  useListRuns,
  useTriggerHandler,
} from "@/views/ingestion/hooks/use-ingestion";

// ---------------------------------------------------------------------------
// Shared handler action hook
// ---------------------------------------------------------------------------

const useHandlerActions = (
  handlerName: string,
  method: string,
): {
  isRunning: boolean;
  activeRun: { id: string; itemsCollected: number } | undefined;
  trigger: () => void;
  cancel: () => void;
  isPending: boolean;
} => {
  const triggerHandler = useTriggerHandler();
  const cancelRun = useCancelHandlerRun();

  const { data: runs } = useListRuns(undefined, {
    refetchInterval: 2_000,
    handlerName,
  });
  const activeRun = runs?.find((r) => r.status === "running");

  const trigger = useCallback(() => {
    triggerHandler.mutate(
      { handlerName, method, key: "" },
      {
        onSuccess: () => toast.success(`${handlerName} started`),
        onError: (err) =>
          toast.error(err instanceof Error ? err.message : `Failed to trigger ${handlerName}`),
      },
    );
  }, [triggerHandler, handlerName, method]);

  const cancel = useCallback(() => {
    if (!activeRun) return;
    cancelRun.mutate(activeRun.id, {
      onSuccess: () => toast.success(`${handlerName} cancelled`),
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel"),
    });
  }, [cancelRun, activeRun, handlerName]);

  return {
    isRunning: !!activeRun,
    activeRun,
    trigger,
    cancel,
    isPending: triggerHandler.isPending || cancelRun.isPending,
  };
};

// ---------------------------------------------------------------------------
// Enrichment row
// ---------------------------------------------------------------------------

const EnrichmentRow = (): React.ReactElement => {
  const { data: status } = useEnrichmentPipelineStatus();
  const actions = useHandlerActions("EnrichmentHandler", "run_cycle");

  const lastRunLabel = status?.lastEnrichmentAt
    ? formatRelativeTime(status.lastEnrichmentAt)
    : undefined;

  return (
    <div className="grid items-center gap-x-2 px-4 py-2.5 text-sm grid-cols-[1fr_auto] sm:grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_7rem]">
      <span className="hidden sm:block" />
      {/* Name + status */}
      <div className="flex min-w-0 items-center gap-2">
        <StatusDot state={actions.isRunning ? "running" : "idle"} animate={actions.isRunning} />
        <span className="truncate font-medium">Enrichments</span>
      </div>

      {/* Stats */}
      <div className="hidden min-w-0 sm:flex flex-wrap items-center gap-x-2.5 gap-y-1">
        {status && Number(status.pendingCount) > 0 && (
          <>
            <Stat label="queued" value={status.pendingCount.toString()} />
          </>
        )}
        {actions.isRunning && actions.activeRun && (
          <>
            {status && Number(status.pendingCount) > 0 && DOT_SEP}
            <Stat label="this run" value={actions.activeRun.itemsCollected.toLocaleString()} />
          </>
        )}
        {!actions.isRunning && lastRunLabel && (
          <span className="text-xs text-muted-foreground">{lastRunLabel}</span>
        )}
      </div>

      {/* Actions */}
      <div className="flex shrink-0 items-center justify-end">
        {actions.isRunning ? (
          <CancelButton onClick={actions.cancel} isPending={actions.isPending} />
        ) : (
          <RunButton onClick={actions.trigger} isPending={actions.isPending} />
        )}
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Embedding row
// ---------------------------------------------------------------------------

const EmbeddingRow = (): React.ReactElement => {
  const { data: embStatus } = useEmbeddingStatus();
  const actions = useHandlerActions("EmbeddingHandler", "run_cycle");

  const lastRunLabel = embStatus?.lastEmbeddedAt
    ? formatRelativeTime(embStatus.lastEmbeddedAt)
    : undefined;

  const coverage = embStatus ? Math.round(embStatus.coveragePercent) : null;

  return (
    <div className="grid items-center gap-x-2 px-4 py-2.5 text-sm grid-cols-[1fr_auto] sm:grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_7rem]">
      <span className="hidden sm:block" />
      {/* Name + status */}
      <div className="flex min-w-0 items-center gap-2">
        <StatusDot state={actions.isRunning ? "running" : "idle"} animate={actions.isRunning} />
        <span className="truncate font-medium">Embeddings</span>
      </div>

      {/* Stats */}
      <div className="hidden min-w-0 sm:flex flex-wrap items-center gap-x-2.5 gap-y-1">
        {coverage !== null && <Stat label="coverage" value={`${coverage}%`} />}
        {embStatus && Number(embStatus.queuedCount) > 0 && (
          <>
            {coverage !== null && DOT_SEP}
            <Stat label="queued" value={embStatus.queuedCount.toString()} />
          </>
        )}
        {!actions.isRunning && lastRunLabel && (
          <span className="text-xs text-muted-foreground">{lastRunLabel}</span>
        )}
      </div>

      {/* Actions */}
      <div className="flex shrink-0 items-center justify-end">
        {actions.isRunning ? (
          <CancelButton onClick={actions.cancel} isPending={actions.isPending} />
        ) : (
          <RunButton onClick={actions.trigger} isPending={actions.isPending} />
        )}
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Main card
// ---------------------------------------------------------------------------

const AiPipelineStatus = (): React.ReactElement => {
  const { isLoading: enrichLoading } = useEnrichmentPipelineStatus();
  const { isLoading: embLoading } = useEmbeddingStatus();

  if (enrichLoading && embLoading) {
    return (
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="flex items-center gap-2 text-sm font-semibold">
            <Brain className="size-4" />
            AI Pipeline
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-2">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader className="pb-3">
        <CardTitle className="flex items-center gap-2 text-sm font-semibold">
          <Brain className="size-4" />
          AI Pipeline
        </CardTitle>
      </CardHeader>
      <CardContent className="px-0 pb-0">
        {/* Column headers — desktop only */}
        <div className="hidden border-b bg-muted/50 px-4 py-2 text-xs font-medium text-muted-foreground sm:grid sm:grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_7rem] sm:items-center sm:gap-x-2">
          <span />
          <span>Handler</span>
          <span>Status</span>
          <span className="text-right">Actions</span>
        </div>
        <EnrichmentRow />
        <EmbeddingRow />
      </CardContent>
    </Card>
  );
};

export { AiPipelineStatus };
