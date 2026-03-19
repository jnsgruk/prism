import { SourceState } from "@ps/api/gen/prism/v1/handlers_pb";
import type { SourceStatus } from "@ps/api/gen/prism/v1/handlers_pb";
import { useMemo } from "react";
import { toast } from "sonner";

import { useEnrichmentPipelineStatus } from "@/views/admin/hooks/use-enrichment";
import {
  useCancelHandlerRun,
  useCancelRun,
  useListRuns,
  useTriggerHandler,
  useTriggerRun,
} from "@/views/ingestion/hooks/use-ingestion";

import type { SourceRowData } from "./source-row";
import { SourceRow } from "./source-row";

const isActive = (s: SourceStatus): boolean =>
  s.state === SourceState.COLLECTING || s.state === SourceState.WAITING;

export const SourceList = ({
  sources,
  onAction,
}: {
  sources: SourceStatus[];
  onAction: () => void;
}): React.ReactElement => {
  const triggerRun = useTriggerRun();
  const cancelRun = useCancelRun();
  const triggerHandler = useTriggerHandler();
  const cancelHandlerRun = useCancelHandlerRun();

  const { data: enrichmentStatus } = useEnrichmentPipelineStatus();
  const { data: enrichmentRuns } = useListRuns(undefined, {
    refetchInterval: 5_000,
    handlerName: "EnrichmentHandler",
  });

  const activeEnrichmentRun = enrichmentRuns?.find((r) => r.status === "running");
  const isEnrichmentRunning = !!activeEnrichmentRun;

  // Sort: active first (by start time), then idle (by last run, most recent first)
  const sortedSources = useMemo(() => {
    const active = sources.filter(isActive);
    const idle = sources.filter((s) => !isActive(s));
    return [...active, ...idle];
  }, [sources]);

  // Build unified row data
  const rows = useMemo((): SourceRowData[] => {
    const sourceRows: SourceRowData[] = sortedSources.map((source) => ({
      kind: "source" as const,
      source,
    }));

    if (!enrichmentStatus) return sourceRows;

    // Insert enrichment row: after active sources, before idle ones
    const activeCount = sortedSources.filter(isActive).length;
    const enrichmentRow: SourceRowData = {
      kind: "enrichment",
      status: enrichmentStatus,
      isRunning: isEnrichmentRunning,
      activeRunId: activeEnrichmentRun?.id,
      itemsThisRun: activeEnrichmentRun?.itemsCollected ?? 0,
    };

    // If enrichment is running, put it with the other active rows
    if (isEnrichmentRunning) {
      sourceRows.splice(activeCount, 0, enrichmentRow);
    } else {
      sourceRows.push(enrichmentRow);
    }

    return sourceRows;
  }, [sortedSources, enrichmentStatus, isEnrichmentRunning, activeEnrichmentRun]);

  const handleTriggerRun = (name: string): void => {
    triggerRun.mutate(name, {
      onSuccess: () => {
        toast.success(`Run triggered for ${name}`);
        onAction();
      },
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to trigger run"),
    });
  };

  const handleCancelRun = (name: string): void => {
    cancelRun.mutate(name, {
      onSuccess: () => {
        toast.success(`Cancelled run for ${name}`);
        onAction();
      },
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel run"),
    });
  };

  const handleTriggerEnrichment = (): void => {
    triggerHandler.mutate(
      { handlerName: "EnrichmentHandler", method: "run_cycle", key: "" },
      {
        onSuccess: () => toast.success("Enrichment run started"),
        onError: (err) =>
          toast.error(err instanceof Error ? err.message : "Failed to trigger enrichment"),
      },
    );
  };

  const handleCancelEnrichment = (runId: string): void => {
    cancelHandlerRun.mutate(runId, {
      onSuccess: () => toast.success("Enrichment run cancelled"),
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel"),
    });
  };

  return (
    <div className="overflow-hidden rounded-lg border bg-card">
      {/* Column headers — desktop only */}
      <div className="hidden border-b bg-muted/50 px-4 py-2 text-xs font-medium text-muted-foreground sm:grid sm:grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_5rem_auto] sm:items-center sm:gap-x-2">
        <span />
        <span>Source</span>
        <span>Progress</span>
        <span className="text-right">Items</span>
        <span className="text-right">Actions</span>
      </div>

      {rows.map((row) => {
        const key = row.kind === "source" ? row.source.name : "_enrichment";
        return (
          <SourceRow
            key={key}
            data={row}
            onTriggerRun={handleTriggerRun}
            onCancelRun={handleCancelRun}
            onTriggerEnrichment={handleTriggerEnrichment}
            onCancelEnrichment={handleCancelEnrichment}
            onAction={onAction}
            isPending={
              triggerRun.isPending ||
              cancelRun.isPending ||
              triggerHandler.isPending ||
              cancelHandlerRun.isPending
            }
          />
        );
      })}
    </div>
  );
};
