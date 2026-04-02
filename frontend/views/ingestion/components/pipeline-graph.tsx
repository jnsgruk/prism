import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { formatRelativeTime } from "@/lib/format";
import { cn } from "@ps/cn";
import { ArrowRight, GitBranch } from "lucide-react";

import {
  SourceState,
  type PipelineInfo,
  type SourceStatus,
} from "@ps/api/gen/canonical/prism/v1/handlers_pb";
import { PipelineActions } from "@/views/ingestion/components/pipeline-actions";
import type { StageData, StageKey } from "@/views/ingestion/components/pipeline-stage";
import { PipelineStage } from "@/views/ingestion/components/pipeline-stage";
import { useCurrentPipeline } from "@/views/ingestion/hooks/use-pipeline";

/** Derive handler status from live source state for in-progress stages. */
const buildSourceStatusMap = (sources: SourceStatus[]): Map<string, string> => {
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
        map.set(s.name, "completed");
    }
  }
  return map;
};

const POLL_INTERVAL_ACTIVE = 2_000;
const POLL_INTERVAL_IDLE = 30_000;

type StagesMap = Record<string, StageData>;

const parseStages = (pipeline: PipelineInfo | undefined): StagesMap => {
  if (!pipeline?.stagesJson) return {};
  try {
    return JSON.parse(pipeline.stagesJson) as StagesMap;
  } catch {
    return {};
  }
};

/** Connector arrow between stages. */
const Arrow = (): React.ReactElement => (
  <ArrowRight className="mx-0.5 size-3.5 shrink-0 text-muted-foreground/50" />
);

/** The main processing branch: metrics → enrichment → embedding → insights. */
const MainBranch = ({
  stages,
  currentStage,
}: {
  stages: StagesMap;
  currentStage: string;
}): React.ReactElement => {
  const mainStages: StageKey[] = ["metrics", "enrichment", "embedding", "insights"];
  return (
    <div className="flex items-start gap-0.5">
      {mainStages.map((key, i) => (
        <div key={key} className="flex items-center">
          {i > 0 && <Arrow />}
          <PipelineStage stageKey={key} stage={stages[key]} isCurrentStage={currentStage === key} />
        </div>
      ))}
    </div>
  );
};

/** The identity resolution branch (runs concurrently with main). */
const IdentityBranch = ({
  stages,
  currentStage,
}: {
  stages: StagesMap;
  currentStage: string;
}): React.ReactElement => (
  <PipelineStage
    stageKey="identity_resolution"
    stage={stages.identity_resolution}
    isCurrentStage={currentStage === "identity_resolution"}
  />
);

const PipelineDAG = ({
  pipeline,
  sourceStatuses,
}: {
  pipeline: PipelineInfo;
  sourceStatuses?: Map<string, string>;
}): React.ReactElement => {
  const stages = parseStages(pipeline);
  const currentStage = pipeline.currentStage;

  return (
    <div className="flex flex-col gap-3 overflow-x-auto">
      {/* Linear spine: ingestion → fork */}
      <div className="flex items-start gap-0.5">
        <PipelineStage
          stageKey="ingestion"
          stage={stages.ingestion}
          isCurrentStage={currentStage === "ingestion"}
          sourceStatuses={sourceStatuses}
        />
        <Arrow />

        {/* Fork after ingestion */}
        <div className="flex flex-col gap-2">
          <MainBranch stages={stages} currentStage={currentStage} />
          <div className="flex items-center gap-0.5 pl-0.5">
            <GitBranch className="size-3 rotate-180 text-muted-foreground/40" />
            <IdentityBranch stages={stages} currentStage={currentStage} />
          </div>
        </div>
      </div>
    </div>
  );
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

const POLL_INTERVAL_BURST = 1_000;

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
  const sourceStatusMap = sources ? buildSourceStatusMap(sources) : undefined;

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
          <Skeleton className="h-20 w-full" />
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
        <CardContent className="overflow-x-auto pb-4">
          <PipelineDAG pipeline={current} sourceStatuses={sourceStatusMap} />
        </CardContent>
      )}
    </Card>
  );
};
