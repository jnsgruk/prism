import { DOT_SEP, Stat } from "@/components/inline-stat";
import { CancelButton, RunButton } from "@/components/run-cancel-buttons";
import { StatusDot, stateStyles } from "@/components/status-dot";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { Brain, ChevronRight, GitPullRequest, MessageSquare, RotateCcw, UserX } from "lucide-react";
import { useMemo, useState } from "react";

import type { SourceStatus } from "@ps/api/gen/prism/v1/handlers_pb";
import { SourceState } from "@ps/api/gen/prism/v1/handlers_pb";
import type { GetEnrichmentPipelineStatusResponse } from "@ps/api/gen/prism/v1/reasoning_pb";
import { cn } from "@ps/cn";

import { formatRelativeTime, formatRelativeTimeIso } from "@/lib/format";
import type { NormalisedProgress, ProgressDetail } from "@/views/ingestion/lib/progress";
import { extractDetail, normaliseProgress, parseProgress } from "@/views/ingestion/lib/progress";
import { BackfillDialog } from "./backfill-dialog";

// ---------------------------------------------------------------------------
// Types for the unified row model
// ---------------------------------------------------------------------------

export type SourceRowData =
  | {
      kind: "source";
      source: SourceStatus;
    }
  | {
      kind: "enrichment";
      status: GetEnrichmentPipelineStatusResponse;
      isRunning: boolean;
      activeRunId?: string;
      itemsThisRun: number;
    };

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
// Inline progress bar
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
      <span className="shrink-0 text-xs tabular-nums text-muted-foreground">
        {progress.percent}%
      </span>
    )}
    <span className="truncate text-xs text-muted-foreground">{progress.label}</span>
  </div>
);

// ---------------------------------------------------------------------------
// Detail expansion — compact inline stats
// ---------------------------------------------------------------------------

const SourceDetail = ({ detail }: { detail: ProgressDetail }): React.ReactElement => {
  const rateLimitLow =
    detail.rateLimit && detail.rateLimit.remaining / detail.rateLimit.limit < 0.1;

  const stats: React.ReactNode[] = [];

  if (detail.prsFetched !== undefined) {
    stats.push(
      <Stat
        key="prs"
        label="PRs"
        value={detail.prsFetched.toLocaleString()}
        icon={<GitPullRequest className="size-3" />}
      />,
    );
  }
  if (detail.reviewsFetched !== undefined) {
    stats.push(
      <Stat
        key="reviews"
        label="reviews"
        value={detail.reviewsFetched.toLocaleString()}
        icon={<MessageSquare className="size-3" />}
      />,
    );
  }
  if (detail.identitiesSkipped !== undefined) {
    stats.push(
      <Stat
        key="skipped"
        label="skipped"
        value={String(detail.identitiesSkipped)}
        icon={<UserX className="size-3" />}
        variant="warning"
      />,
    );
  }
  if (detail.rateLimit) {
    stats.push(
      <Stat
        key="rate"
        label="API calls left"
        value={`${detail.rateLimit.remaining.toLocaleString()}/${detail.rateLimit.limit.toLocaleString()}`}
        variant={rateLimitLow ? "danger" : undefined}
      />,
    );
  }

  return (
    <div className="border-b bg-muted/40 px-4 py-2.5">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
        {stats.map((stat, i) => (
          <span key={i} className="flex items-center gap-x-3">
            {i > 0 && DOT_SEP}
            {stat}
          </span>
        ))}
      </div>
      {detail.statusMessage && (
        <p className="mt-1 truncate text-xs italic text-muted-foreground">{detail.statusMessage}</p>
      )}
    </div>
  );
};

const EnrichmentDetail = ({
  status,
}: {
  status: GetEnrichmentPipelineStatusResponse;
}): React.ReactElement => (
  <div className="flex items-center border-b bg-muted/40 px-4 py-2.5">
    <div className="flex items-center gap-x-3">
      <Stat label="queued" value={status.pendingCount.toString()} />
      {DOT_SEP}
      <Stat label="enriched" value={status.totalEnrichments.toString()} />
    </div>
  </div>
);

// ---------------------------------------------------------------------------
// Row icon cell — first column
// ---------------------------------------------------------------------------

const RowIcon = ({
  isSource,
  hasDetail,
  isActive,
  expanded,
}: {
  isSource: boolean;
  hasDetail: boolean;
  isActive: boolean;
  expanded: boolean;
}): React.ReactElement => {
  if (hasDetail && isActive) {
    return (
      <CollapsibleTrigger className="flex items-center justify-center">
        <ChevronRight
          className={cn(
            "size-3.5 text-muted-foreground transition-transform",
            expanded && "rotate-90",
          )}
        />
      </CollapsibleTrigger>
    );
  }
  if (!isSource) {
    return <Brain className="size-3.5 text-muted-foreground" />;
  }
  return <span />;
};

// ---------------------------------------------------------------------------
// Main row component
// ---------------------------------------------------------------------------

export const SourceRow = ({
  data,
  onTriggerRun,
  onCancelRun,
  onTriggerEnrichment,
  onCancelEnrichment,
  onAction,
  isPending,
}: {
  data: SourceRowData;
  onTriggerRun?: (name: string) => void;
  onCancelRun?: (name: string) => void;
  onTriggerEnrichment?: () => void;
  onCancelEnrichment?: (runId: string) => void;
  onAction?: () => void;
  isPending?: boolean;
}): React.ReactElement => {
  const [expanded, setExpanded] = useState(false);
  const [showBackfill, setShowBackfill] = useState(false);

  // Derive unified fields
  const isSource = data.kind === "source";
  const name = isSource ? data.source.name : "Enrichments";

  let stateKey: string;
  let items: number;
  if (isSource) {
    stateKey = stateFromEnum(data.source.state);
    items = data.source.itemsCollected;
  } else {
    stateKey = data.isRunning ? "running" : "idle";
    items = data.isRunning ? data.itemsThisRun : Number(data.status.totalEnrichments);
  }

  const stateLabel = stateStyles[stateKey]?.label ?? "Idle";
  const isActive = stateKey === "collecting" || stateKey === "waiting" || stateKey === "running";

  const progress = useMemo((): NormalisedProgress | null => {
    if (!isActive) return null;
    if (isSource) {
      const raw = parseProgress(data.source.progressJson);
      return normaliseProgress(data.source.sourceType, raw);
    }
    // Enrichment: derive from pending + total
    const pending = Number(data.status.pendingCount);
    const total = pending + data.itemsThisRun;
    if (total > 0 && data.isRunning) {
      return {
        percent: Math.round((data.itemsThisRun / total) * 100),
        label: `${data.itemsThisRun.toLocaleString()}/${total.toLocaleString()}`,
      };
    }
    return { percent: null, label: "Processing" };
  }, [isSource, isActive, data]);

  const detail = useMemo((): ProgressDetail | null => {
    if (!isSource || !isActive) return null;
    const raw = parseProgress(data.source.progressJson);
    return extractDetail(raw);
  }, [isSource, isActive, data]);

  const hasDetail = isSource ? !!detail : true;
  const lastRun = isSource ? data.source.lastRun : undefined;
  const lastRunIso = !isSource ? data.status.lastEnrichmentAt : undefined;

  let relativeTime: string;
  if (isSource) {
    relativeTime = lastRun ? formatRelativeTime(lastRun) : "Never";
  } else {
    relativeTime = lastRunIso ? formatRelativeTimeIso(lastRunIso) : "Never";
  }

  // Render actions for this row
  const renderActions = (): React.ReactElement => {
    const pending = !!isPending;
    if (isSource && isActive) {
      return <CancelButton onClick={() => onCancelRun?.(data.source.name)} isPending={pending} />;
    }
    if (isSource) {
      return (
        <>
          <RunButton onClick={() => onTriggerRun?.(data.source.name)} isPending={pending} />
          <TooltipProvider>
            <Tooltip>
              <TooltipTrigger
                render={
                  <Button
                    variant="ghost"
                    size="sm"
                    className="h-7 px-1.5"
                    onClick={() => setShowBackfill(true)}
                  />
                }
              >
                <RotateCcw className="size-3.5" />
              </TooltipTrigger>
              <TooltipContent>Backfill</TooltipContent>
            </Tooltip>
          </TooltipProvider>
        </>
      );
    }
    // Enrichment
    if (data.isRunning && data.activeRunId) {
      const runId = data.activeRunId;
      return <CancelButton onClick={() => onCancelEnrichment?.(runId)} isPending={pending} />;
    }
    return <RunButton onClick={() => onTriggerEnrichment?.()} isPending={pending} />;
  };

  return (
    <>
      <Collapsible open={expanded} onOpenChange={setExpanded}>
        <div
          className={cn(
            "group grid items-center gap-x-2 border-b px-4 py-2.5 text-sm last:border-b-0",
            "grid-cols-[1rem_1fr_auto_auto_auto]",
            "sm:grid-cols-[1rem_minmax(8rem,1fr)_minmax(12rem,2fr)_6rem_7rem]",
          )}
        >
          {/* Expand chevron / icon */}
          <RowIcon
            isSource={isSource}
            hasDetail={hasDetail}
            isActive={isActive}
            expanded={expanded}
          />

          {/* Name + status */}
          <div className="flex min-w-0 items-center gap-2">
            <StatusDot state={stateKey} animate={isActive} />
            <span className="truncate font-medium">{name}</span>
            <span className="hidden text-xs text-muted-foreground sm:inline">{stateLabel}</span>
          </div>

          {/* Progress or last run */}
          <div className="hidden min-w-0 sm:block">
            {isActive && progress ? (
              <InlineProgress progress={progress} />
            ) : (
              <span className="text-xs text-muted-foreground">{relativeTime}</span>
            )}
          </div>

          {/* Items */}
          <span className="hidden text-right tabular-nums sm:block">{items.toLocaleString()}</span>

          {/* Actions */}
          <div className="flex shrink-0 items-center justify-end gap-1">{renderActions()}</div>
        </div>

        {/* Expandable detail */}
        <CollapsibleContent>
          {isSource && detail && <SourceDetail detail={detail} />}
          {!isSource && <EnrichmentDetail status={data.status} />}
        </CollapsibleContent>
      </Collapsible>

      {/* Mobile progress — shown below the row on small screens */}
      {isActive && progress && (
        <div className="border-b px-4 pb-2 sm:hidden">
          <InlineProgress progress={progress} />
        </div>
      )}

      {isSource && (
        <BackfillDialog
          sourceName={data.source.name}
          open={showBackfill}
          onOpenChange={setShowBackfill}
          onAction={onAction}
        />
      )}
    </>
  );
};
