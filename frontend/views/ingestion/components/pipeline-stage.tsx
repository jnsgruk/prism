import { StatusDot } from "@/components/status-dot";
import { cn } from "@ps/cn";
import { Check, Loader2, X } from "lucide-react";

/** Shape of a single handler within a stage (from stages JSONB). */
export type StageHandler = {
  name: string;
  status: string;
  items?: number;
  error?: string;
};

/** Shape of a single stage entry from the stages JSONB. */
export type StageData = {
  status: string;
  started_at?: string;
  completed_at?: string;
  branch?: string;
  handlers?: StageHandler[];
};

/** All stage keys in pipeline order. */
export const STAGE_ORDER = [
  "team_sync",
  "ingestion",
  "metrics",
  "enrichment",
  "embedding",
  "insights",
  "identity_resolution",
] as const;

export type StageKey = (typeof STAGE_ORDER)[number];

const STAGE_LABELS: Record<StageKey, string> = {
  team_sync: "Team Sync",
  ingestion: "Ingestion",
  metrics: "Metrics",
  enrichment: "Enrichment",
  embedding: "Embedding",
  insights: "Insights",
  identity_resolution: "Identity Resolution",
};

const statusIcon = (status: string): React.ReactElement => {
  switch (status) {
    case "completed":
      return <Check className="size-3.5 text-emerald-500" />;
    case "running":
      return <Loader2 className="size-3.5 animate-spin text-blue-500" />;
    case "failed":
      return <X className="size-3.5 text-destructive" />;
    case "skipped":
      return <span className="size-3.5 text-center text-xs text-muted-foreground">—</span>;
    default:
      return <span className="size-3.5 rounded-full border border-muted-foreground/30" />;
  }
};

const statusDotState = (status: string): string => {
  switch (status) {
    case "completed":
      return "idle";
    case "running":
      return "running";
    case "failed":
      return "error";
    default:
      return "idle";
  }
};

const HandlerRow = ({ handler }: { handler: StageHandler }): React.ReactElement => (
  <div className="flex items-center gap-2 text-xs">
    {statusIcon(handler.status)}
    <span className="truncate">{handler.name}</span>
    {handler.items != null && handler.items > 0 && (
      <span className="tabular-nums text-muted-foreground">{handler.items.toLocaleString()}</span>
    )}
    {handler.error && (
      <span className="truncate text-destructive" title={handler.error}>
        {handler.error}
      </span>
    )}
  </div>
);

export const PipelineStage = ({
  stageKey,
  stage,
  isCurrentStage,
}: {
  stageKey: StageKey;
  stage: StageData | undefined;
  isCurrentStage: boolean;
}): React.ReactElement => {
  const status = stage?.status ?? "pending";
  const handlers = stage?.handlers ?? [];

  return (
    <div
      className={cn(
        "flex min-w-28 flex-col gap-1.5 rounded-md border px-3 py-2",
        isCurrentStage && status === "running" && "border-blue-500/50 bg-blue-500/5",
        status === "failed" && "border-destructive/50 bg-destructive/5",
        status === "completed" && "border-emerald-500/30",
        status === "skipped" && "opacity-50",
      )}
    >
      {/* Stage header */}
      <div className="flex items-center gap-2">
        <StatusDot state={statusDotState(status)} animate={status === "running"} />
        <span className="text-xs font-medium">{STAGE_LABELS[stageKey]}</span>
      </div>

      {/* Handler detail rows (only for multi-handler stages) */}
      {handlers.length > 1 && (
        <div className="space-y-1 pl-1">
          {handlers.map((h) => (
            <HandlerRow key={h.name} handler={h} />
          ))}
        </div>
      )}

      {/* Single-handler status summary */}
      {handlers.length === 1 && handlers[0] && handlers[0].status !== "pending" && (
        <div className="pl-1">
          <HandlerRow handler={handlers[0]} />
        </div>
      )}
    </div>
  );
};
