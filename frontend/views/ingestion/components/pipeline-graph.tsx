import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { formatRelativeTime } from "@/lib/format";
import { cn } from "@ps/cn";
import { GitBranch } from "lucide-react";

import { SourceState, type SourceStatus } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { PipelineActions } from "@/views/ingestion/components/pipeline-actions";
import { PipelineDAGFlow } from "@/views/ingestion/components/pipeline-dag-flow";
import { useCurrentPipeline } from "@/views/ingestion/hooks/use-pipeline";

/**
 * Derive handler status from live source state for in-progress stages.
 *
 * COLLECTING/WAITING -> "running", ERROR -> "failed".
 * IDLE -> "completed" only if the source's lastRun is after the pipeline started
 * (proving it ran during this pipeline), otherwise left out (stays "pending").
 */
const buildSourceStatusMap = (
  sources: SourceStatus[],
  pipelineStartedSeconds: bigint | undefined,
): Map<string, string> => {
  const map = new Map<string, string>();
  for (const s of sources) {
    switch (s.state) {
      case SourceState.COLLECTING:
      case SourceState.WAITING:
        map.set(s.name, "running");
        break;
      case SourceState.ERROR:
        map.set(s.name, "failed");
        break;
      default:
        // IDLE: only mark completed if the source's last run finished after the pipeline started
        if (pipelineStartedSeconds && s.lastRun && s.lastRun.seconds > pipelineStartedSeconds) {
          map.set(s.name, "completed");
        }
        break;
    }
  }
  return map;
};

import {
  POLL_INTERVAL_ACTIVE,
  POLL_INTERVAL_BURST,
  POLL_INTERVAL_IDLE,
} from "@/views/ingestion/lib/constants";

const StatusBadge = ({ status }: { status: string }): React.ReactElement => {
  const styles: Record<string, string> = {
    running: "bg-blue-500/10 text-blue-600",
    completed: "bg-emerald-500/10 text-emerald-600",
    completed_with_warnings: "bg-amber-500/10 text-amber-600",
    failed: "bg-destructive/10 text-destructive",
    cancelled: "bg-muted text-muted-foreground",
  };
  return (
    <span
      className={cn(
        "rounded-full px-2 py-0.5 text-[10px] font-medium uppercase",
        styles[status] ?? "bg-muted text-muted-foreground",
      )}
    >
      {status.replace(/_/g, " ")}
    </span>
  );
};

export const PipelineGraph = ({
  onAction,
  sources,
  isBursting,
}: {
  onAction: () => void;
  sources?: SourceStatus[];
  /** When true, poll at 1s to pick up newly-triggered pipelines quickly. */
  isBursting?: boolean;
}): React.ReactElement => {
  const { current, isLoading } = useCurrentPipeline({
    refetchInterval: (query) => {
      if (isBursting) return POLL_INTERVAL_BURST;
      const pipeline = query.state.data?.current;
      if (pipeline?.status === "running") return POLL_INTERVAL_ACTIVE;
      return POLL_INTERVAL_IDLE;
    },
  });

  const isRunning = current?.status === "running";
  const sourceStatusMap = sources
    ? buildSourceStatusMap(sources, current?.startedAt?.seconds)
    : undefined;

  if (isLoading) {
    return (
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="flex items-center gap-2 text-sm font-semibold">
            <GitBranch className="size-4" />
            Pipeline
          </CardTitle>
        </CardHeader>
        <CardContent>
          <Skeleton className="h-[280px] w-full" />
        </CardContent>
      </Card>
    );
  }

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <CardTitle className="flex items-center gap-2 text-sm font-semibold">
              <GitBranch className="size-4" />
              Pipeline
            </CardTitle>
            {current && <StatusBadge status={current.status} />}
            {current?.startedAt && (
              <span className="text-xs text-muted-foreground">
                {formatRelativeTime(current.startedAt)}
              </span>
            )}
          </div>
          <PipelineActions pipelineId={current?.id} isRunning={isRunning} onAction={onAction} />
        </div>
      </CardHeader>

      {current && (
        <CardContent className="pb-4">
          <PipelineDAGFlow pipeline={current} sourceStatuses={sourceStatusMap} />
        </CardContent>
      )}
    </Card>
  );
};
