import { Button } from "@/components/ui/button";
import { Loader2, Play } from "lucide-react";

import type { SourceStatus } from "@ps/api/gen/prism/v1/handlers_pb";
import { SourceState } from "@ps/api/gen/prism/v1/handlers_pb";

import { formatRelativeTime } from "@/lib/format";

const isActive = (s: SourceStatus): boolean =>
  s.state === SourceState.COLLECTING || s.state === SourceState.WAITING;

export const IngestionSummary = ({
  sources,
  onRunAll,
  isPending,
}: {
  sources: SourceStatus[];
  onRunAll: () => void;
  isPending: boolean;
}): React.ReactElement => {
  const running = sources.filter(isActive);
  const idle = sources.filter((s) => !isActive(s));
  const errorCount = sources.filter((s) => s.state === SourceState.ERROR).length;
  const hasActive = running.length > 0;

  const totalItems = running.reduce((sum, s) => sum + s.itemsCollected, 0);

  // Find most recent last run across idle sources
  const lastRunTs = idle.reduce<{ seconds: bigint } | undefined>((latest, s) => {
    if (!s.lastRun) return latest;
    if (!latest || s.lastRun.seconds > latest.seconds) return s.lastRun;
    return latest;
  }, undefined);

  return (
    <div className="flex items-center gap-4 rounded-lg border bg-card px-4 py-3">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm">
          {hasActive ? (
            <>
              <span>
                <span className="font-medium tabular-nums">{running.length}</span>{" "}
                <span className="text-muted-foreground">running</span>
              </span>
              <span className="text-muted-foreground">·</span>
              <span>
                <span className="font-medium tabular-nums">{idle.length}</span>{" "}
                <span className="text-muted-foreground">idle</span>
              </span>
              {errorCount > 0 && (
                <>
                  <span className="text-muted-foreground">·</span>
                  <span className="text-destructive">
                    <span className="font-medium tabular-nums">{errorCount}</span> error
                  </span>
                </>
              )}
              <span className="text-muted-foreground">·</span>
              <span>
                <span className="font-medium tabular-nums">{totalItems.toLocaleString()}</span>{" "}
                <span className="text-muted-foreground">items collected</span>
              </span>
            </>
          ) : (
            <span className="text-muted-foreground">
              All sources idle
              {lastRunTs && <> · Last run {formatRelativeTime(lastRunTs)}</>}
            </span>
          )}
        </div>
      </div>

      <Button variant="outline" size="sm" onClick={onRunAll} disabled={isPending}>
        {isPending ? (
          <Loader2 className="mr-1.5 size-3.5 animate-spin" />
        ) : (
          <Play className="mr-1.5 size-3.5" />
        )}
        Run All
      </Button>
    </div>
  );
};
