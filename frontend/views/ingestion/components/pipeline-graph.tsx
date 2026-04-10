import { Skeleton } from "@/components/ui/skeleton";
import { PipelineDAGFlow } from "@/views/ingestion/components/pipeline-dag-flow";
import { useCurrentPipeline } from "@/views/ingestion/hooks/use-pipeline";
import { POLL_INTERVAL_ACTIVE, POLL_INTERVAL_BURST, POLL_INTERVAL_IDLE } from "@/views/ingestion/lib/constants";

import type { SourceStatus } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { SourceState } from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { cn } from "@ps/cn";

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

export { StatusBadge };

export const usePipelineState = (options?: {
  isBursting?: boolean;
}): {
  current: ReturnType<typeof useCurrentPipeline>["current"];
  isLoading: boolean;
  isRunning: boolean;
} => {
  const { current, isLoading } = useCurrentPipeline({
    refetchInterval: (query) => {
      if (options?.isBursting) return POLL_INTERVAL_BURST;
      const pipeline = query.state.data?.current;
      if (pipeline?.status === "running") return POLL_INTERVAL_ACTIVE;
      return POLL_INTERVAL_IDLE;
    },
  });

  return { current, isLoading, isRunning: current?.status === "running" };
};

export const PipelineDAG = ({
  sources,
  isBursting,
}: {
  sources?: SourceStatus[];
  isBursting?: boolean;
}): React.ReactElement => {
  const { current, isLoading } = usePipelineState({ isBursting });

  const sourceStatusMap = sources ? buildSourceStatusMap(sources, current?.startedAt?.seconds) : undefined;

  if (isLoading) {
    return <Skeleton className="h-[200px] w-full" />;
  }

  if (!current) {
    return (
      <p className="py-8 text-center text-sm text-muted-foreground">
        No pipeline runs yet. Click Run Pipeline to start.
      </p>
    );
  }

  return <PipelineDAGFlow pipeline={current} sourceStatuses={sourceStatusMap} />;
};
