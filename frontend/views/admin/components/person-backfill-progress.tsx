import { DataTable, type DataTableColumnDef } from "@/components/data-table/data-table";
import { RunCoverage } from "@/components/run-coverage";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { formatTimestamp } from "@/lib/format";
import { statusConfig, defaultStatus } from "@/lib/run-status";
import { StatusBadge } from "@/views/ingestion/components/pipeline-graph";
import { normaliseProgress, parseProgress } from "@/views/ingestion/lib/progress";

import { RunStatus } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { HandlerRun, PipelineInfo, PipelineRunSummary } from "@ps/api/gen/canonical/prism/v1/handlers_pb";

const runColumns: DataTableColumnDef<HandlerRun>[] = [
  { accessorKey: "sourceName", header: "Source", enableSorting: false },
  {
    id: "status",
    header: "Status",
    enableSorting: false,
    cell: ({ row }) => {
      const style = statusConfig[row.original.status] ?? defaultStatus;
      return <Badge variant={style.variant}>{style.label}</Badge>;
    },
  },
  {
    id: "progress",
    header: "Progress",
    enableSorting: false,
    cell: ({ row }) => {
      const run = row.original;
      const progress = normaliseProgress(
        run.handlerName.includes("Github") ? "github" : "other",
        parseProgress(run.progressJson),
      );
      return (
        <div className="min-w-0 space-y-1 text-xs">
          <p>
            {run.itemsCollected.toLocaleString()} items{run.status === RunStatus.RUNNING && ` · ${progress.label}`}
          </p>
          {run.status === RunStatus.RUNNING && progress.pauseNote && (
            <p className="text-muted-foreground">{progress.pauseNote}</p>
          )}
          {run.errorMessage && <p className="break-words text-destructive">{run.errorMessage}</p>}
          <RunCoverage progressJson={run.progressJson} />
        </div>
      );
    },
  },
];

const historyColumns: DataTableColumnDef<PipelineRunSummary>[] = [
  {
    id: "started",
    header: "Started",
    enableSorting: false,
    cell: ({ row }) => formatTimestamp(row.original.pipeline?.startedAt),
  },
  {
    id: "status",
    header: "Status",
    enableSorting: false,
    cell: ({ row }) => <StatusBadge status={row.original.pipeline?.status ?? "unknown"} />,
  },
  {
    id: "since",
    header: "Since",
    enableSorting: false,
    cell: ({ row }) => row.original.pipeline?.sinceDate ?? "—",
  },
];

export const PersonBackfillProgress = ({
  pipeline,
  runs,
}: {
  pipeline: PipelineInfo;
  runs: HandlerRun[];
}): React.ReactElement => (
  <div className="min-w-0 space-y-3">
    <div className="flex flex-wrap items-center gap-2">
      <StatusBadge status={pipeline.status} />
      <span className="text-sm">{pipeline.currentStage.replace(/_/g, " ") || "Waiting for dispatch"}</span>
    </div>
    <p className="break-all text-xs text-muted-foreground">Pipeline {pipeline.id}</p>
    {pipeline.selectedSources.length > 0 && (
      <p className="text-sm text-muted-foreground">
        Sources:{" "}
        {pipeline.selectedSources
          .map((source) => `${source.sourceName}${source.instance ? ` (${source.instance})` : ""}`)
          .join(", ")}
      </p>
    )}
    {pipeline.status === "cancelling" && (
      <p className="text-sm">Cancellation requested. Waiting for owned work to stop.</p>
    )}
    {pipeline.error && (
      <Alert variant="destructive" className="break-words">
        {pipeline.error}
      </Alert>
    )}
    {runs.length ? (
      <div className="overflow-x-auto rounded-md border">
        <DataTable columns={runColumns} data={runs} />
      </div>
    ) : (
      <p className="text-sm text-muted-foreground">No source runs recorded yet.</p>
    )}
  </div>
);

export const PersonBackfillHistory = ({
  summaries,
  onSelect,
}: {
  summaries: PipelineRunSummary[];
  onSelect: (id: string) => void;
}): React.ReactElement => (
  <div className="min-w-0 space-y-2">
    <p className="text-sm font-medium">Recent backfills</p>
    <div className="overflow-x-auto rounded-md border">
      <DataTable
        columns={historyColumns}
        data={summaries}
        onRowClick={(summary) => {
          if (summary.pipeline) onSelect(summary.pipeline.id);
        }}
      />
    </div>
  </div>
);
