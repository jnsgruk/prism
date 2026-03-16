import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  Clock,
  GitPullRequest,
  Loader2,
  MessageSquare,
  Pause,
  Play,
  RotateCcw,
  Search,
  Square,
  UserX,
} from "lucide-react";
import { useMemo, useState } from "react";
import { toast } from "sonner";

import type { SourceStatus } from "@ps/api/gen/prism/v1/handlers_pb";
import { SourceState } from "@ps/api/gen/prism/v1/handlers_pb";
import { cn } from "@ps/cn";

import { HandlerRunsTable } from "@/views/ingestion/components/ingestion-runs-table";
import { useCancelRun, useListRuns, useTriggerRun } from "@/views/ingestion/hooks/use-ingestion";
import { BackfillDialog } from "./backfill-dialog";

export interface RunProgress {
  phase?: string;
  repos_total?: number;
  repos_completed?: number;
  current_repo?: string;
  prs_fetched?: number;
  reviews_fetched?: number;
  identities_skipped?: number;
  search_users_total?: number;
  search_users_completed?: number;
  rate_limit_remaining?: number;
  rate_limit_limit?: number;
}

const stateConfig: Record<
  SourceState,
  {
    label: string;
    variant: "default" | "secondary" | "destructive" | "outline";
    icon: React.ReactNode;
  }
> = {
  [SourceState.IDLE]: {
    label: "Idle",
    variant: "secondary",
    icon: <CheckCircle2 className="size-3.5" />,
  },
  [SourceState.COLLECTING]: {
    label: "Collecting",
    variant: "default",
    icon: <Loader2 className="size-3.5 animate-spin" />,
  },
  [SourceState.WAITING]: {
    label: "Waiting",
    variant: "outline",
    icon: <Pause className="size-3.5" />,
  },
  [SourceState.ERROR]: {
    label: "Error",
    variant: "destructive",
    icon: <AlertCircle className="size-3.5" />,
  },
  [SourceState.UNSPECIFIED]: {
    label: "Unknown",
    variant: "outline",
    icon: <Clock className="size-3.5" />,
  },
};

const formatTimestamp = (ts?: { seconds: bigint }): string => {
  if (!ts) return "Never";
  const date = new Date(Number(ts.seconds) * 1000);
  return (
    date.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
    ", " +
    date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
  );
};

const formatRelativeTime = (ts?: { seconds: bigint }): string => {
  if (!ts) return "";
  const now = Date.now();
  const then = Number(ts.seconds) * 1000;
  const diffMs = now - then;
  const diffMin = Math.floor(diffMs / 60_000);
  if (diffMin < 1) return "just now";
  if (diffMin < 60) return `${String(diffMin)}m ago`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24) return `${String(diffHours)}h ago`;
  const diffDays = Math.floor(diffHours / 24);
  return `${String(diffDays)}d ago`;
};

const phaseLabel = (phase?: string): string => {
  switch (phase) {
    case "team_repos":
      return "Fetching repos";
    case "member_search":
      return "Searching members";
    case "complete":
      return "Finalising";
    default:
      return "Starting";
  }
};

// ---------------------------------------------------------------------------
// Progress section — shown inline when a run is active
// ---------------------------------------------------------------------------

const ProgressSection = ({ progress }: { progress: RunProgress }): React.ReactElement => {
  const isSearch = progress.phase === "member_search";
  const reposTotal = progress.repos_total ?? 0;
  const reposCompleted = progress.repos_completed ?? 0;
  const repoPercent = reposTotal > 0 ? Math.round((reposCompleted / reposTotal) * 100) : 0;

  const searchTotal = progress.search_users_total ?? 0;
  const searchCompleted = progress.search_users_completed ?? 0;

  const rateLimitPercent =
    progress.rate_limit_limit && progress.rate_limit_limit > 0
      ? Math.round(((progress.rate_limit_remaining ?? 0) / progress.rate_limit_limit) * 100)
      : null;

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between">
        <span className="text-xs font-medium">{phaseLabel(progress.phase)}</span>
        {rateLimitPercent !== null && (
          <span
            className={cn(
              "text-xs tabular-nums",
              rateLimitPercent < 10 ? "text-destructive" : "text-muted-foreground",
            )}
          >
            {progress.rate_limit_remaining?.toLocaleString()}/
            {progress.rate_limit_limit?.toLocaleString()} API calls left
          </span>
        )}
      </div>

      {!isSearch && reposTotal > 0 && (
        <div className="space-y-1">
          <div className="flex justify-between text-xs text-muted-foreground">
            <span>
              {reposCompleted}/{reposTotal} repos
            </span>
            <span>{repoPercent}%</span>
          </div>
          <div className="h-1.5 overflow-hidden rounded-full bg-muted">
            <div
              className="h-full rounded-full bg-primary transition-all duration-300"
              style={{ width: `${String(repoPercent)}%` }}
            />
          </div>
          {progress.current_repo && (
            <p className="truncate text-xs text-muted-foreground">{progress.current_repo}</p>
          )}
        </div>
      )}

      {isSearch && searchTotal > 0 && (
        <div className="flex items-center gap-1 text-xs text-muted-foreground">
          <Search className="size-3" />
          <span>
            {searchCompleted}/{searchTotal} users searched
          </span>
        </div>
      )}

      <div className="flex gap-3 text-xs text-muted-foreground">
        {(progress.prs_fetched ?? 0) > 0 && (
          <span className="flex items-center gap-1">
            <GitPullRequest className="size-3" />
            {progress.prs_fetched?.toLocaleString()} PRs
          </span>
        )}
        {(progress.reviews_fetched ?? 0) > 0 && (
          <span className="flex items-center gap-1">
            <MessageSquare className="size-3" />
            {progress.reviews_fetched?.toLocaleString()} reviews
          </span>
        )}
        {(progress.identities_skipped ?? 0) > 0 && (
          <span className="flex items-center gap-1 text-amber-600 dark:text-amber-400">
            <UserX className="size-3" />
            {progress.identities_skipped} skipped
          </span>
        )}
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Source row — full-width card with expandable run history
// ---------------------------------------------------------------------------

export const SourceStatusRow = ({
  source,
  onAction,
}: {
  source: SourceStatus;
  onAction?: () => void;
}): React.ReactElement => {
  const triggerRun = useTriggerRun();
  const cancelRun = useCancelRun();
  const [showBackfill, setShowBackfill] = useState(false);
  const [expanded, setExpanded] = useState(false);
  const config = stateConfig[source.state] ?? stateConfig[SourceState.UNSPECIFIED];
  const isCollecting = source.state === SourceState.COLLECTING;

  let runsInterval: number | false = false;
  if (expanded) runsInterval = isCollecting ? 3_000 : 30_000;

  const { data: runs } = useListRuns(source.name, {
    refetchInterval: runsInterval,
    handlerName: "GithubIngestionHandler",
  });

  const progress = useMemo((): RunProgress | null => {
    if (!isCollecting || !source.progressJson) return null;
    try {
      return JSON.parse(source.progressJson) as RunProgress;
    } catch {
      return null;
    }
  }, [isCollecting, source.progressJson]);

  const handleTriggerRun = (): void => {
    triggerRun.mutate(source.name, {
      onSuccess: () => {
        toast.success(`Run triggered for ${source.name}`);
        onAction?.();
      },
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to trigger run"),
    });
  };

  const handleCancelRun = (): void => {
    cancelRun.mutate(source.name, {
      onSuccess: () => {
        toast.success(`Cancelled run for ${source.name}`);
        onAction?.();
      },
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel run"),
    });
  };

  return (
    <>
      <div className="rounded-lg border bg-card">
        {/* Main row */}
        <div className="flex items-start gap-6 p-5">
          {/* Left: identity */}
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-3">
              <h3 className="text-sm font-semibold">{source.name}</h3>
              <Badge variant={config.variant} className="gap-1 text-xs">
                {config.icon}
                {config.label}
              </Badge>
            </div>
            <p className="mt-0.5 text-xs text-muted-foreground">{source.sourceType}</p>
          </div>

          {/* Centre: stats */}
          <div className="hidden gap-8 sm:flex">
            <div className="text-right">
              <p className="text-xs text-muted-foreground">
                {isCollecting ? "Started" : "Last run"}
              </p>
              <p className={cn("text-sm font-medium", !source.lastRun && "text-muted-foreground")}>
                {source.lastRun ? formatRelativeTime(source.lastRun) : "Never"}
              </p>
              {source.lastRun && (
                <p className="text-xs text-muted-foreground">{formatTimestamp(source.lastRun)}</p>
              )}
            </div>
            <div className="text-right">
              <p className="text-xs text-muted-foreground">
                {isCollecting ? "Items so far" : "Items collected"}
              </p>
              <p className="text-sm font-medium tabular-nums">
                {source.itemsCollected.toLocaleString()}
              </p>
            </div>
          </div>

          {/* Right: actions */}
          <div className="flex shrink-0 items-center gap-2">
            {isCollecting ? (
              <Button
                variant="destructive"
                size="sm"
                onClick={handleCancelRun}
                disabled={cancelRun.isPending}
              >
                {cancelRun.isPending ? (
                  <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                ) : (
                  <Square className="mr-1.5 size-3.5" />
                )}
                Cancel
              </Button>
            ) : (
              <>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={handleTriggerRun}
                  disabled={triggerRun.isPending}
                >
                  {triggerRun.isPending ? (
                    <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                  ) : (
                    <Play className="mr-1.5 size-3.5" />
                  )}
                  Run Now
                </Button>
                <Button variant="outline" size="sm" onClick={() => setShowBackfill(true)}>
                  <RotateCcw className="mr-1.5 size-3.5" />
                  Backfill
                </Button>
              </>
            )}
            <Button
              variant="ghost"
              size="sm"
              onClick={() => setExpanded(!expanded)}
              className="ml-1 px-2"
            >
              <ChevronDown
                className={cn("size-4 transition-transform duration-200", expanded && "rotate-180")}
              />
            </Button>
          </div>
        </div>

        {/* Progress bar — shown when collecting */}
        {progress && (
          <>
            <Separator />
            <div className="px-5 py-3">
              <ProgressSection progress={progress} />
            </div>
          </>
        )}

        {/* Mobile stats — visible on small screens only */}
        <div className="flex gap-6 border-t px-5 py-3 sm:hidden">
          <div>
            <p className="text-xs text-muted-foreground">{isCollecting ? "Started" : "Last run"}</p>
            <p className={cn("text-sm font-medium", !source.lastRun && "text-muted-foreground")}>
              {source.lastRun ? formatRelativeTime(source.lastRun) : "Never"}
            </p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">
              {isCollecting ? "Items so far" : "Items collected"}
            </p>
            <p className="text-sm font-medium tabular-nums">
              {source.itemsCollected.toLocaleString()}
            </p>
          </div>
        </div>

        {/* Expanded: run history */}
        {expanded && (
          <>
            <Separator />
            <div className="p-5">
              <h4 className="mb-3 text-xs font-medium text-muted-foreground uppercase tracking-wide">
                Run History
              </h4>
              <HandlerRunsTable runs={runs ?? []} />
            </div>
          </>
        )}
      </div>

      <BackfillDialog sourceName={source.name} open={showBackfill} onOpenChange={setShowBackfill} />
    </>
  );
};
