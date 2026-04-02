import type { SourceStatus } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { useMemo } from "react";
import { toast } from "sonner";

import { useCancelRun, useTriggerRun } from "@/views/ingestion/hooks/use-ingestion";
import { isActive } from "@/views/ingestion/lib/constants";

import { SourceRow } from "./source-row";

export const SourceList = ({
  sources,
  onAction,
}: {
  sources: SourceStatus[];
  onAction: () => void;
}): React.ReactElement => {
  const triggerRun = useTriggerRun();
  const cancelRun = useCancelRun();

  // Sort: active first (by start time), then idle (by last run, most recent first)
  const sortedSources = useMemo(() => {
    const active = sources.filter(isActive);
    const idle = sources.filter((s) => !isActive(s));
    return [...active, ...idle];
  }, [sources]);

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
      <div className="hidden border-b bg-muted/50 px-4 py-2 text-xs font-medium text-muted-foreground sm:grid sm:grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_6rem_7rem] sm:items-center sm:gap-x-2">
        <span />
        <span>Source</span>
        <span>Progress</span>
        <span className="text-right">Items</span>
        <span className="text-right">Actions</span>
      </div>

      {sortedSources.map((source) => (
        <SourceRow
          key={source.name}
          source={source}
          onTriggerRun={handleTriggerRun}
          onCancelRun={handleCancelRun}
          onAction={onAction}
          isPending={triggerRun.isPending || cancelRun.isPending}
        />
      ))}
    </div>
  );
};
