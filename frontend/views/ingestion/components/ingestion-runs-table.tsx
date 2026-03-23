import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ColumnDef } from "@tanstack/react-table";
import { useCallback, useMemo, useState } from "react";
import { toast } from "sonner";

import type { HandlerRun } from "@ps/api/gen/canonical/prism/v1/handlers_pb";

import { RunHistoryCard } from "@/components/run-history-card";
import { formatDuration, formatTimestamp } from "@/lib/format";
import { defaultStatus, statusConfig } from "@/lib/run-status";
import { useCancelHandlerRun } from "@/views/ingestion/hooks/use-ingestion";

const SOURCE_DISPLAY_NAMES: Record<string, string> = {
  _enrichment: "Enrichment",
  _embedding: "Embedding",
  _system: "System",
};

const displaySourceName = (name: string): string => SOURCE_DISPLAY_NAMES[name] ?? name;

const columns: ColumnDef<HandlerRun, unknown>[] = [
  {
    accessorKey: "sourceName",
    header: "Source",
    cell: ({ row }) => (
      <span className="font-medium">{displaySourceName(row.original.sourceName)}</span>
    ),
  },
  {
    accessorKey: "startedAt",
    header: "Started",
    cell: ({ row }) => <span className="text-xs">{formatTimestamp(row.original.startedAt)}</span>,
  },
  {
    id: "duration",
    header: "Duration",
    cell: ({ row }) => (
      <span className="text-xs">
        {formatDuration(row.original.startedAt, row.original.completedAt)}
      </span>
    ),
  },
  {
    accessorKey: "itemsCollected",
    header: () => <span className="block text-right">Items</span>,
    cell: ({ row }) => (
      <span className="block text-right tabular-nums">
        {row.original.itemsCollected.toLocaleString()}
      </span>
    ),
  },
  {
    accessorKey: "status",
    header: "Status",
    cell: ({ row }) => {
      const cfg = statusConfig[row.original.status] ?? defaultStatus;
      return (
        <Badge variant={cfg.variant} className="gap-1">
          {cfg.icon}
          {cfg.label}
        </Badge>
      );
    },
  },
];

export const RunHistoryPanel = ({
  runs,
  sourceNames,
}: {
  runs: HandlerRun[];
  sourceNames: string[];
}): React.ReactElement => {
  const cancelRun = useCancelHandlerRun();
  const [sourceFilter, setSourceFilter] = useState<string>("all");

  const handleCancel = useCallback(
    (runId: string) => {
      cancelRun.mutate(runId, {
        onSuccess: () => toast.success("Run cancelled"),
        onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel"),
      });
    },
    [cancelRun],
  );

  const filteredRuns = useMemo(() => {
    if (sourceFilter === "all") return runs;
    return runs.filter((r) => r.sourceName === sourceFilter);
  }, [runs, sourceFilter]);

  const entityDropdown = (
    <Select value={sourceFilter} onValueChange={(v) => setSourceFilter(v ?? "all")}>
      <SelectTrigger size="sm">
        <SelectValue />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="all">All sources</SelectItem>
        {sourceNames.map((name) => (
          <SelectItem key={name} value={name}>
            {displaySourceName(name)}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );

  return (
    <RunHistoryCard
      runs={filteredRuns}
      columns={columns}
      entityDropdown={entityDropdown}
      entityFilter={sourceFilter}
      runTitle={(run) => displaySourceName(run.sourceName)}
      onCancel={handleCancel}
      cancelPending={cancelRun.isPending}
    />
  );
};
