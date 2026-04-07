import { DOT_SEP, Stat } from "@/components/inline-stat";
import { StatusDot } from "@/components/status-dot";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { MoreHorizontal, Play, Square } from "lucide-react";
import { useCallback } from "react";
import { toast } from "sonner";

import { RunStatus } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { formatRelativeTime } from "@/lib/format";
import { useEmbeddingStatus } from "@/lib/hooks/use-embeddings";
import { useEnrichmentPipelineStatus } from "@/lib/hooks/use-enrichment";
import { useCancelHandlerRun, useListRuns, useTriggerHandler } from "@/lib/hooks/use-ingestion";

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
  const activeRun = runs?.find((r) => r.status === RunStatus.RUNNING);

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
// Shared overflow menu for AI handler rows
// ---------------------------------------------------------------------------

const AiHandlerMenu = ({
  actions,
}: {
  actions: ReturnType<typeof useHandlerActions>;
}): React.ReactElement => (
  <div className="flex shrink-0 items-center justify-end">
    <DropdownMenu>
      <DropdownMenuTrigger
        render={
          <Button
            variant="ghost"
            size="sm"
            className="h-7 w-7 p-0 opacity-0 group-hover:opacity-100 data-popup-open:opacity-100 sm:opacity-0"
          />
        }
      >
        <MoreHorizontal className="size-4" />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" side="bottom">
        {actions.isRunning ? (
          <DropdownMenuItem onClick={actions.cancel}>
            <Square className="size-3.5" />
            Cancel
          </DropdownMenuItem>
        ) : (
          <DropdownMenuItem onClick={actions.trigger}>
            <Play className="size-3.5" />
            Run
          </DropdownMenuItem>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  </div>
);

// ---------------------------------------------------------------------------
// Enrichment row
// ---------------------------------------------------------------------------

export const EnrichmentRow = (): React.ReactElement => {
  const { data: status } = useEnrichmentPipelineStatus();
  const actions = useHandlerActions("EnrichmentHandler", "run_cycle");

  const lastRunLabel = status?.lastEnrichmentAt
    ? formatRelativeTime(status.lastEnrichmentAt)
    : undefined;

  return (
    <div className="group grid items-center gap-x-2 px-4 py-2.5 text-sm grid-cols-[1rem_1fr_auto_auto] sm:grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_6rem_2rem]">
      <span />
      {/* Name + status */}
      <div className="flex min-w-0 items-center gap-2">
        <StatusDot state={actions.isRunning ? "running" : "idle"} animate={actions.isRunning} />
        <span className="truncate font-medium">Enrichments</span>
      </div>

      {/* Stats */}
      <div className="hidden min-w-0 sm:flex flex-wrap items-center gap-x-2.5 gap-y-1">
        {status && Number(status.pendingCount) > 0 && (
          <Stat label="queued" value={status.pendingCount.toString()} />
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

      {/* Items */}
      <span className="hidden text-right tabular-nums sm:block">
        {status ? Number(status.totalEnrichments).toLocaleString() : "—"}
      </span>

      {/* Actions */}
      <AiHandlerMenu actions={actions} />
    </div>
  );
};

// ---------------------------------------------------------------------------
// Embedding row
// ---------------------------------------------------------------------------

export const EmbeddingRow = (): React.ReactElement => {
  const { data: embStatus } = useEmbeddingStatus();
  const actions = useHandlerActions("EmbeddingHandler", "run_cycle");

  const lastRunLabel = embStatus?.lastEmbeddedAt
    ? formatRelativeTime(embStatus.lastEmbeddedAt)
    : undefined;

  const coverage = embStatus ? Math.round(embStatus.coveragePercent) : null;

  return (
    <div className="group grid items-center gap-x-2 px-4 py-2.5 text-sm grid-cols-[1rem_1fr_auto_auto] sm:grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_6rem_2rem]">
      <span />
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

      {/* Items */}
      <span className="hidden text-right tabular-nums sm:block">
        {embStatus ? Number(embStatus.embeddedCount).toLocaleString() : "—"}
      </span>

      {/* Actions */}
      <AiHandlerMenu actions={actions} />
    </div>
  );
};
