import { StatusDot, stateStyles } from "@/components/status-dot";
import { formatRelativeTime } from "@/lib/format";
import { platformKey } from "@/lib/proto-display";
import type { NormalisedProgress } from "@/views/ingestion/lib/progress";
import { normaliseProgress, parseProgress } from "@/views/ingestion/lib/progress";
import { useMemo } from "react";

import type { SourceStatus } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { SourceState } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { cn } from "@ps/cn";

import { SourceOverflowMenu } from "./source-overflow-menu";

// ---------------------------------------------------------------------------
// State helpers
// ---------------------------------------------------------------------------

const stateFromEnum = (state: SourceState): string => {
  switch (state) {
    case SourceState.COLLECTING:
      return "collecting";
    case SourceState.WAITING:
      return "waiting";
    case SourceState.IDLE:
      return "idle";
    case SourceState.ERROR:
      return "error";
    default:
      return "idle";
  }
};

// ---------------------------------------------------------------------------
// Inline progress bar + rate limit
// ---------------------------------------------------------------------------

const InlineProgress = ({ progress }: { progress: NormalisedProgress }): React.ReactElement => (
  <div className="flex min-w-0 items-center gap-2">
    <div className="h-1.5 w-20 shrink-0 overflow-hidden rounded-full bg-muted">
      {progress.percent !== null ? (
        <div
          className="h-full rounded-full bg-primary transition-all duration-300"
          style={{ width: `${String(Math.min(100, progress.percent))}%` }}
        />
      ) : (
        <div className="h-full w-1/3 animate-pulse rounded-full bg-primary/60" />
      )}
    </div>
    {progress.percent !== null && (
      <span className="shrink-0 text-xs tabular-nums text-muted-foreground">{progress.percent}%</span>
    )}
    <span className="truncate text-xs text-muted-foreground">{progress.label}</span>
    {progress.pauseNote && (
      <span className="shrink-0 text-xs whitespace-nowrap text-destructive">({progress.pauseNote})</span>
    )}
  </div>
);

// ---------------------------------------------------------------------------
// Main row component
// ---------------------------------------------------------------------------

export const SourceRow = ({
  source,
  sourceId,
  enabled = true,
  onToggleEnabled,
}: {
  source: SourceStatus;
  sourceId?: string;
  enabled?: boolean;
  onToggleEnabled?: (sourceId: string, enabled: boolean) => void;
}): React.ReactElement => {
  const stateKey = enabled ? stateFromEnum(source.state) : "disabled";
  const stateLabel = enabled ? (stateStyles[stateKey]?.label ?? "Idle") : "Disabled";
  const isActive = enabled && stateKey === "collecting";

  const progress = useMemo((): NormalisedProgress | null => {
    if (!isActive) return null;
    const raw = parseProgress(source.progressJson);
    return normaliseProgress(platformKey(source.sourceType), raw);
  }, [isActive, source.progressJson, source.sourceType]);

  const relativeTime = source.lastRun ? formatRelativeTime(source.lastRun) : "Never";

  return (
    <div>
      <div
        className={cn(
          "group grid items-center gap-x-2 px-4 py-2.5 text-sm",
          "grid-cols-[1fr_auto_auto]",
          "sm:grid-cols-[14rem_1fr_2rem]",
          !enabled && "opacity-50",
        )}
      >
        {/* Name + status */}
        <div className="flex min-w-0 items-center gap-2">
          <StatusDot state={enabled ? stateKey : "pending"} animate={isActive} />
          <span className="truncate font-medium">{source.name}</span>
          <span className="hidden text-xs text-muted-foreground sm:inline">{stateLabel}</span>
        </div>

        {/* Progress or last run */}
        <div className="hidden min-w-0 sm:block">
          {isActive && progress && <InlineProgress progress={progress} />}
          {!isActive && enabled && <span className="text-xs text-muted-foreground">{relativeTime}</span>}
        </div>

        {/* Overflow menu */}
        <div className="flex shrink-0 items-center justify-end">
          <SourceOverflowMenu sourceId={sourceId} enabled={enabled} onToggleEnabled={onToggleEnabled} />
        </div>
      </div>

      {/* Mobile progress — shown below the row on small screens */}
      {isActive && progress && (
        <div className="px-4 pb-2 sm:hidden">
          <InlineProgress progress={progress} />
        </div>
      )}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Disabled source row — rendered from SourceConfig when no SourceStatus exists
// ---------------------------------------------------------------------------

export const DisabledSourceRow = ({
  name,
  sourceId,
  onToggleEnabled,
}: {
  name: string;
  sourceId: string;
  onToggleEnabled?: (sourceId: string, enabled: boolean) => void;
}): React.ReactElement => (
  <div
    className={cn(
      "group grid items-center gap-x-2 px-4 py-2.5 text-sm text-muted-foreground",
      "grid-cols-[1fr_auto_auto]",
      "sm:grid-cols-[14rem_1fr_2rem]",
    )}
  >
    <div className="flex min-w-0 items-center gap-2">
      <StatusDot state="pending" animate={false} />
      <span className="truncate font-medium">{name}</span>
      <span className="hidden text-xs sm:inline">Disabled</span>
    </div>
    <div className="hidden sm:block" />
    <div className="flex shrink-0 items-center justify-end">
      <SourceOverflowMenu sourceId={sourceId} enabled={false} onToggleEnabled={onToggleEnabled} />
    </div>
  </div>
);
