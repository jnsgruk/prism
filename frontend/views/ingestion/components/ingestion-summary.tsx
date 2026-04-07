import type { SourceStatus } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { SourceState } from "@ps/api/gen/canonical/prism/v1/handlers_pb";

import { formatRelativeTime } from "@/lib/format";
import { isActive } from "@/views/ingestion/lib/constants";

/** Inline text summary — sits next to the card title. */
export const IngestionSummary = ({
  sources,
  disabledCount = 0,
}: {
  sources: SourceStatus[];
  disabledCount?: number;
}): React.ReactElement => {
  const running = sources.filter(isActive);
  const idle = sources.filter((s) => !isActive(s));
  const errorCount = sources.filter((s) => s.state === SourceState.ERROR).length;
  const hasActive = running.length > 0;

  const totalItems = running.reduce((sum, s) => sum + s.itemsCollected, 0);

  const lastRunTs = idle.reduce<{ seconds: bigint } | undefined>((latest, s) => {
    if (!s.lastRun) return latest;
    if (!latest || s.lastRun.seconds > latest.seconds) return s.lastRun;
    return latest;
  }, undefined);

  return (
    <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
      {hasActive ? (
        <>
          <span>
            <span className="font-medium tabular-nums text-foreground">{running.length}</span>{" "}
            running
          </span>
          <span>·</span>
          <span>
            <span className="font-medium tabular-nums text-foreground">{idle.length}</span> idle
          </span>
          {errorCount > 0 && (
            <>
              <span>·</span>
              <span className="text-destructive">
                <span className="font-medium tabular-nums">{errorCount}</span> error
              </span>
            </>
          )}
          {disabledCount > 0 && (
            <>
              <span>·</span>
              <span>
                <span className="font-medium tabular-nums text-foreground">{disabledCount}</span>{" "}
                disabled
              </span>
            </>
          )}
          <span>·</span>
          <span>
            <span className="font-medium tabular-nums text-foreground">
              {totalItems.toLocaleString()}
            </span>{" "}
            items
          </span>
        </>
      ) : (
        <span>
          {disabledCount > 0 ? `${idle.length} idle · ${disabledCount} disabled` : "All idle"}
          {lastRunTs && <> · Last run {formatRelativeTime(lastRunTs)}</>}
        </span>
      )}
    </div>
  );
};
