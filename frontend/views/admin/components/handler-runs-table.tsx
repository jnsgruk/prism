import { useMemo, useState } from "react";
import { Badge } from "@/components/ui/badge";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ColumnDef } from "@tanstack/react-table";

import type { HandlerInfo, HandlerRun } from "@ps/api/gen/prism/v1/handlers_pb";

import { RunHistoryCard } from "@/components/run-history-card";
import { formatDuration, formatTimestamp } from "@/lib/format";
import { defaultStatus, statusConfig } from "@/lib/run-status";

const displayName = (name: string): string => name.replace("Handler", "");

const handlerRunColumns: ColumnDef<HandlerRun, unknown>[] = [
  {
    accessorKey: "handlerName",
    header: "Handler",
    cell: ({ row }): React.ReactElement => {
      const source = row.original.sourceName;
      const suffix = source && source !== "_system" ? ` \u2014 ${source}` : "";
      return (
        <div>
          <span className="font-medium">
            {displayName(row.original.handlerName)}.{row.original.handlerMethod}
          </span>
          {suffix && <span className="text-muted-foreground">{suffix}</span>}
        </div>
      );
    },
  },
  {
    accessorKey: "startedAt",
    header: "Started",
    cell: ({ row }): React.ReactElement => (
      <span className="text-xs">{formatTimestamp(row.original.startedAt)}</span>
    ),
  },
  {
    id: "duration",
    header: "Duration",
    cell: ({ row }): React.ReactElement => (
      <span className="text-xs">
        {formatDuration(row.original.startedAt, row.original.completedAt)}
      </span>
    ),
  },
  {
    accessorKey: "itemsCollected",
    header: () => <span className="block text-right">Items</span>,
    cell: ({ row }): React.ReactElement => (
      <span className="block text-right tabular-nums">
        {row.original.itemsCollected.toLocaleString()}
      </span>
    ),
  },
  {
    accessorKey: "status",
    header: "Status",
    cell: ({ row }): React.ReactElement => {
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

export const HandlerRunsCard = ({
  runs,
  handlers,
  onCancelRun,
  cancelPending,
}: {
  runs: HandlerRun[];
  handlers: HandlerInfo[];
  onCancelRun: (runId: string) => void;
  cancelPending: boolean;
}): React.ReactElement => {
  const [handlerFilter, setHandlerFilter] = useState<string>("all");

  const { ingestionHandlers, systemHandlers } = useMemo(() => {
    const ingestionNames = new Set(["EnrichmentHandler"]);
    return {
      ingestionHandlers: handlers.filter((h) => h.requiresKey || ingestionNames.has(h.name)),
      systemHandlers: handlers.filter((h) => !h.requiresKey && !ingestionNames.has(h.name)),
    };
  }, [handlers]);

  const filteredRuns = useMemo(() => {
    if (handlerFilter === "all") return runs;
    return runs.filter((r) => r.handlerName === handlerFilter);
  }, [runs, handlerFilter]);

  const entityDropdown = (
    <Select value={handlerFilter} onValueChange={(v) => v !== null && setHandlerFilter(v)}>
      <SelectTrigger size="sm">
        <SelectValue placeholder="All handlers" />
      </SelectTrigger>
      <SelectContent>
        <SelectItem value="all">All handlers</SelectItem>
        {ingestionHandlers.length > 0 && (
          <SelectGroup>
            <SelectLabel>Ingestion</SelectLabel>
            {ingestionHandlers.map((h) => (
              <SelectItem key={h.name} value={h.name}>
                {displayName(h.name)}
              </SelectItem>
            ))}
          </SelectGroup>
        )}
        {systemHandlers.length > 0 && (
          <SelectGroup>
            <SelectLabel>System</SelectLabel>
            {systemHandlers.map((h) => (
              <SelectItem key={h.name} value={h.name}>
                {displayName(h.name)}
              </SelectItem>
            ))}
          </SelectGroup>
        )}
      </SelectContent>
    </Select>
  );

  return (
    <RunHistoryCard
      runs={filteredRuns}
      columns={handlerRunColumns}
      entityDropdown={entityDropdown}
      entityFilter={handlerFilter}
      runTitle={(run) => `${displayName(run.handlerName)}.${run.handlerMethod}`}
      runDescription={(run) =>
        run.sourceName === "_system" ? "Run details" : `Source: ${run.sourceName}`
      }
      onCancel={onCancelRun}
      cancelPending={cancelPending}
      excludeRunningByDefault
    />
  );
};
