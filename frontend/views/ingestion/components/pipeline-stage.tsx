import { StatusDot } from "@/components/status-dot";
import { Check, Loader2, X } from "lucide-react";

import { cn } from "@ps/cn";

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
  "ingestion",
  "metrics",
  "enrichment",
  "embedding",
  "insights",
  "identity_resolution",
] as const;

export type StageKey = (typeof STAGE_ORDER)[number];

const STAGE_LABELS: Record<StageKey, string> = {
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
    case "pending":
    case "cancelled":
    case "skipped":
      return "pending";
    default:
      return "pending";
  }
};

const HandlerRow = ({ handler }: { handler: StageHandler }): React.ReactElement => (
  <div className="flex min-w-0 items-center gap-2 text-xs">
    <span className="shrink-0">{statusIcon(handler.status)}</span>
    <span className="shrink-0">{handler.name}</span>
    {handler.items != null && handler.items > 0 && (
      <span className="shrink-0 tabular-nums text-muted-foreground">{handler.items.toLocaleString()}</span>
    )}
    {handler.error && (
      <span className="min-w-0 truncate text-destructive" title={handler.error}>
        {handler.error}
      </span>
    )}
  </div>
);

export const PipelineStage = ({
  stageKey,
  stage,
  isCurrentStage,
  sourceStatuses,
}: {
  stageKey: StageKey;
  stage: StageData | undefined;
  isCurrentStage: boolean;
  /** Live source statuses — used to enrich ingestion handler display while the stage is running. */
  sourceStatuses?: Map<string, string>;
}): React.ReactElement => {
  const status = stage?.status ?? "pending";
  const rawHandlers = stage?.handlers ?? [];

  // When the ingestion stage is running, the stages JSONB only updates after
  // ALL handlers finish (join_all). Enrich "pending" handlers with live source
  // statuses so completed/running sources show real-time progress.
  const handlers =
    stageKey === "ingestion" && status === "running" && sourceStatuses
      ? rawHandlers.map((h) => {
          if (h.status !== "pending") return h;
          const derived = sourceStatuses.get(h.name);
          return derived ? { ...h, status: derived } : h;
        })
      : rawHandlers;

  return (
    <div
      className={cn(
        "flex flex-col gap-1.5 overflow-hidden rounded-md border px-3 py-2",
        stageKey === "ingestion" ? "min-w-28 max-w-52" : "w-40",
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
