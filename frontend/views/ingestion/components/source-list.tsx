import type { SourceStatus } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { useMemo } from "react";
import { toast } from "sonner";

import { useCancelRun, useTriggerRun } from "@/views/ingestion/hooks/use-ingestion";
import { isActive } from "@/views/ingestion/lib/constants";

import { Separator } from "@/components/ui/separator";
import { EmbeddingRow, EnrichmentRow } from "./ai-handler-row";
import { SourceRow } from "./source-row";

export type SourceConfigInfo = { id: string; enabled: boolean };

export const SourceList = ({
  sources,
  sourceConfigs,
  onAction,
  onToggleEnabled,
}: {
  sources: SourceStatus[];
  sourceConfigs?: Map<string, SourceConfigInfo>;
  onAction: () => void;
  onToggleEnabled?: (sourceId: string, enabled: boolean) => void;
}): React.ReactElement => {
  const triggerRun = useTriggerRun();
  const cancelRun = useCancelRun();

  // Sort: active first, then idle, then disabled
  const sortedSources = useMemo(() => {
    const active = sources.filter(
      (s) => isActive(s) && sourceConfigs?.get(s.name)?.enabled !== false,
    );
    const idle = sources.filter(
      (s) => !isActive(s) && sourceConfigs?.get(s.name)?.enabled !== false,
    );
    const disabled = sources.filter((s) => sourceConfigs?.get(s.name)?.enabled === false);
    return [...active, ...idle, ...disabled];
  }, [sources, sourceConfigs]);

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

  return (
    <div>
      {/* Column headers — desktop only */}
      <div className="hidden border-b bg-muted/50 px-4 py-2 text-xs font-medium text-muted-foreground sm:grid sm:grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_6rem_2rem] sm:items-center sm:gap-x-2">
        <span />
        <span>Source</span>
        <span>Progress</span>
        <span className="text-right">Items</span>
        <span />
      </div>

      {sortedSources.map((source) => {
        const config = sourceConfigs?.get(source.name);
        return (
          <SourceRow
            key={source.name}
            source={source}
            sourceId={config?.id}
            enabled={config?.enabled ?? true}
            onTriggerRun={handleTriggerRun}
            onCancelRun={handleCancelRun}
            onToggleEnabled={onToggleEnabled}
            onAction={onAction}
          />
        );
      })}

      {/* AI handler rows */}
      <Separator />
      <EnrichmentRow />
      <EmbeddingRow />
    </div>
  );
};
