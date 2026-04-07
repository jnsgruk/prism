import type { SourceStatus } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { useMemo } from "react";
import { toast } from "sonner";

import { useCancelRun, useTriggerRun } from "@/views/ingestion/hooks/use-ingestion";
import { isActive } from "@/views/ingestion/lib/constants";

import { EmbeddingRow, EnrichmentRow } from "./ai-handler-row";
import { DisabledSourceRow, SourceRow } from "./source-row";

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

  const isDisabled = (name: string): boolean => sourceConfigs?.get(name)?.enabled === false;

  // Enabled sources only: active first, then idle
  const sortedSources = useMemo(() => {
    const enabled = sources.filter((s) => !isDisabled(s.name));
    const active = enabled.filter(isActive);
    const idle = enabled.filter((s) => !isActive(s));
    return [...active, ...idle];
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sources, sourceConfigs]);

  // Disabled sources: from configs (may or may not have a SourceStatus entry)
  const disabledSources = useMemo(() => {
    if (!sourceConfigs) return [];
    return [...sourceConfigs.entries()]
      .filter(([, cfg]) => !cfg.enabled)
      .map(([name, cfg]) => ({ name, id: cfg.id }));
  }, [sourceConfigs]);

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
      <div className="hidden border-b bg-muted/50 px-4 py-2 text-xs font-medium text-muted-foreground sm:grid sm:grid-cols-[1rem_minmax(8rem,1fr)_1fr_2rem] sm:items-center sm:gap-x-2">
        <span />
        <span>Source</span>
        <span>Status</span>
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

      {disabledSources.map((ds) => (
        <DisabledSourceRow
          key={ds.name}
          name={ds.name}
          sourceId={ds.id}
          onToggleEnabled={onToggleEnabled}
        />
      ))}

      {/* AI handler rows */}
      <EnrichmentRow />
      <EmbeddingRow />
    </div>
  );
};
