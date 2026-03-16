import { useCallback, useEffect, useMemo, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ColumnDef } from "@tanstack/react-table";
import { AlertCircle, Ban, CheckCircle2, Cog, Loader2, Play } from "lucide-react";
import { toast } from "sonner";

import type { HandlerInfo, HandlerRun } from "@ps/api/gen/prism/v1/handlers_pb";
import { useListSources } from "@ps/hooks/use-config";

import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import {
  useListHandlers,
  useListRuns,
  useTriggerHandler,
} from "@/views/ingestion/hooks/use-ingestion";

type StatusStyle = {
  label: string;
  variant: "default" | "secondary" | "destructive";
  icon: React.ReactNode;
};

const defaultStatus: StatusStyle = {
  label: "Running",
  variant: "default",
  icon: <Loader2 className="size-3 animate-spin" />,
};

const statusConfig: Record<string, StatusStyle> = {
  completed: {
    label: "Completed",
    variant: "secondary",
    icon: <CheckCircle2 className="size-3" />,
  },
  failed: {
    label: "Failed",
    variant: "destructive",
    icon: <AlertCircle className="size-3" />,
  },
  cancelled: {
    label: "Cancelled",
    variant: "secondary",
    icon: <Ban className="size-3" />,
  },
  running: defaultStatus,
};

const formatTimestamp = (ts?: { seconds: bigint }): string => {
  if (!ts) return "\u2014";
  const date = new Date(Number(ts.seconds) * 1000);
  return (
    date.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
    " " +
    date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false })
  );
};

const formatDuration = (start?: { seconds: bigint }, end?: { seconds: bigint }): string => {
  if (!start || !end) return "\u2014";
  const diffSec = Number(end.seconds - start.seconds);
  if (diffSec < 60) return `${String(diffSec)}s`;
  const min = Math.floor(diffSec / 60);
  const sec = diffSec % 60;
  return `${String(min)}m ${String(sec)}s`;
};

const TriggerHandlerDialog = ({
  handler,
  open,
  onOpenChange,
}: {
  handler: HandlerInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const { data: sources } = useListSources();
  const trigger = useTriggerHandler();
  const [method, setMethod] = useState(handler.methods[0] ?? "");
  const [sourceName, setSourceName] = useState("");

  const needsSource = handler.requiresKey;

  const handleTrigger = (): void => {
    if ((needsSource && !sourceName) || !method) return;
    trigger.mutate(
      { handlerName: handler.name, method, key: sourceName },
      {
        onSuccess: (resp) => {
          toast.success(
            `Triggered ${handler.name}.${method} (${resp.invocationId.slice(0, 12)}...)`,
          );
          onOpenChange(false);
        },
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : "Failed to trigger handler");
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Trigger {handler.name}</DialogTitle>
          <DialogDescription>{handler.description}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-2">
            <label className="text-sm font-medium">Method</label>
            <Select value={method} onValueChange={(v) => v !== null && setMethod(v)}>
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {handler.methods.map((m) => (
                  <SelectItem key={m} value={m}>
                    {m}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          {needsSource && (
            <div className="space-y-2">
              <label className="text-sm font-medium">Source</label>
              <Select value={sourceName} onValueChange={(v) => v !== null && setSourceName(v)}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="Select a source..." />
                </SelectTrigger>
                <SelectContent>
                  {sources?.map((s) => (
                    <SelectItem key={s.id} value={s.name}>
                      {s.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          )}
        </div>

        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button
            onClick={handleTrigger}
            disabled={(needsSource && !sourceName) || !method || trigger.isPending}
          >
            {trigger.isPending ? (
              <Loader2 className="mr-1 size-4 animate-spin" />
            ) : (
              <Play className="mr-1 size-4" />
            )}
            Trigger
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

const HandlerCard = ({ handler }: { handler: HandlerInfo }): React.ReactElement => {
  const [triggerOpen, setTriggerOpen] = useState(false);

  return (
    <>
      <div className="flex items-center justify-between rounded-lg border px-4 py-3">
        <div className="flex items-center gap-3">
          <Cog className="size-5 text-muted-foreground" />
          <div>
            <p className="text-sm font-medium">{handler.name}</p>
            <p className="text-xs text-muted-foreground">{handler.description}</p>
            <div className="mt-1 flex gap-1">
              {handler.methods.map((m) => (
                <Badge key={m} variant="outline" className="text-xs">
                  {m}
                </Badge>
              ))}
            </div>
          </div>
        </div>
        <Button variant="outline" size="sm" onClick={() => setTriggerOpen(true)}>
          <Play className="mr-1 size-3" />
          Trigger
        </Button>
      </div>
      <TriggerHandlerDialog handler={handler} open={triggerOpen} onOpenChange={setTriggerOpen} />
    </>
  );
};

const formatFullTimestamp = (ts?: { seconds: bigint }): string => {
  if (!ts) return "—";
  return new Date(Number(ts.seconds) * 1000).toLocaleString();
};

const handlerRunColumns: ColumnDef<HandlerRun, unknown>[] = [
  {
    accessorKey: "handlerName",
    header: "Handler",
    cell: ({ row }) => <span className="font-medium">{row.original.handlerName}</span>,
  },
  {
    accessorKey: "handlerMethod",
    header: "Method",
    cell: ({ row }) => <span className="text-xs">{row.original.handlerMethod}</span>,
  },
  {
    accessorKey: "sourceName",
    header: "Source",
    cell: ({ row }) => (
      <span className="text-xs">
        {row.original.sourceName === "_system" ? "—" : row.original.sourceName}
      </span>
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

const RunDetailDialog = ({
  run,
  open,
  onOpenChange,
}: {
  run: HandlerRun;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const runConfig = statusConfig[run.status] ?? defaultStatus;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {run.handlerName}.{run.handlerMethod}
          </DialogTitle>
          <DialogDescription>
            {run.sourceName === "_system" ? "Run details" : `Source: ${run.sourceName}`}
          </DialogDescription>
        </DialogHeader>
        <div className="space-y-3 text-sm">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <p className="text-xs text-muted-foreground">Status</p>
              <Badge variant={runConfig.variant} className="mt-1 gap-1">
                {runConfig.icon}
                {runConfig.label}
              </Badge>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Items collected</p>
              <p className="font-medium">{run.itemsCollected.toLocaleString()}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Started</p>
              <p>{formatFullTimestamp(run.startedAt)}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Completed</p>
              <p>{formatFullTimestamp(run.completedAt)}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Duration</p>
              <p>{formatDuration(run.startedAt, run.completedAt)}</p>
            </div>
            {run.rateLimitWaitsSeconds > 0 && (
              <div>
                <p className="text-xs text-muted-foreground">Rate limit waits</p>
                <p>{String(run.rateLimitWaitsSeconds)}s</p>
              </div>
            )}
          </div>
          {run.errorMessage && (
            <div>
              <p className="text-xs text-muted-foreground">Error</p>
              <p className="mt-1 rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">
                {run.errorMessage}
              </p>
            </div>
          )}
        </div>
      </DialogContent>
    </Dialog>
  );
};

type StatusFilter = "all" | "completed" | "failed" | "cancelled" | "running";

const HandlerRunsTable = ({
  runs,
  handlers,
}: {
  runs: HandlerRun[];
  handlers: HandlerInfo[];
}): React.ReactElement => {
  const [selectedRun, setSelectedRun] = useState<HandlerRun | null>(null);
  const [handlerFilter, setHandlerFilter] = useState<string>("all");
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const [pageSize, setPageSize] = useState(25);
  const [pageIndex, setPageIndex] = useState(0);

  useEffect(() => {
    setPageIndex(0);
  }, [handlerFilter, statusFilter, pageSize]);

  const filteredRuns = useMemo(() => {
    let result = runs;
    if (handlerFilter !== "all") {
      result = result.filter((r) => r.handlerName === handlerFilter);
    }
    if (statusFilter !== "all") {
      result = result.filter((r) => r.status === statusFilter);
    }
    return result;
  }, [runs, handlerFilter, statusFilter]);

  const totalCount = filteredRuns.length;
  const pageRuns = filteredRuns.slice(pageIndex * pageSize, (pageIndex + 1) * pageSize);
  const hasNextPage = (pageIndex + 1) * pageSize < totalCount;

  const handleNextPage = useCallback(() => {
    setPageIndex((i) => i + 1);
  }, []);

  const handlePrevPage = useCallback(() => {
    setPageIndex((i) => Math.max(0, i - 1));
  }, []);

  const handlePageSizeChange = useCallback((size: number) => {
    setPageSize(size);
  }, []);

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center gap-3">
        <div className="flex items-center gap-1">
          <Button
            variant={handlerFilter === "all" ? "default" : "outline"}
            size="sm"
            onClick={() => setHandlerFilter("all")}
          >
            All handlers
          </Button>
          {handlers.map((h) => (
            <Button
              key={h.name}
              variant={handlerFilter === h.name ? "default" : "outline"}
              size="sm"
              onClick={() => setHandlerFilter(h.name)}
            >
              {h.name.replace("Handler", "")}
            </Button>
          ))}
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant={statusFilter === "all" ? "default" : "outline"}
            size="sm"
            onClick={() => setStatusFilter("all")}
          >
            All
          </Button>
          <Button
            variant={statusFilter === "completed" ? "default" : "outline"}
            size="sm"
            onClick={() => setStatusFilter("completed")}
          >
            Completed
          </Button>
          <Button
            variant={statusFilter === "failed" ? "default" : "outline"}
            size="sm"
            onClick={() => setStatusFilter("failed")}
          >
            Failed
          </Button>
          <Button
            variant={statusFilter === "running" ? "default" : "outline"}
            size="sm"
            onClick={() => setStatusFilter("running")}
          >
            Running
          </Button>
        </div>
      </div>

      <DataTable columns={handlerRunColumns} data={pageRuns} onRowClick={setSelectedRun} />

      <DataTablePagination
        totalCount={totalCount}
        pageSize={pageSize}
        pageIndex={pageIndex}
        hasNextPage={hasNextPage}
        onPageSizeChange={handlePageSizeChange}
        onPreviousPage={handlePrevPage}
        onNextPage={handleNextPage}
      />

      {selectedRun && (
        <RunDetailDialog
          run={selectedRun}
          open={!!selectedRun}
          onOpenChange={(open) => {
            if (!open) setSelectedRun(null);
          }}
        />
      )}
    </div>
  );
};

export const HandlersTab = (): React.ReactElement => {
  const { data: handlers, isLoading: handlersLoading, error: handlersError } = useListHandlers();
  const { data: runs } = useListRuns(undefined, { refetchInterval: 5000 });

  return (
    <div className="space-y-6 pt-4">
      {/* Registered handlers */}
      <div>
        <p className="mb-3 text-sm text-muted-foreground">
          Registered Restate handlers and their available methods.
        </p>

        {handlersLoading && <p className="text-sm text-muted-foreground">Loading handlers...</p>}

        {handlersError && (
          <Alert variant="destructive">
            <AlertCircle className="size-4" />
            Failed to load handlers.
          </Alert>
        )}

        {handlers && (
          <div className="space-y-2">
            {handlers.map((h) => (
              <HandlerCard key={h.name} handler={h} />
            ))}
          </div>
        )}
      </div>

      {/* Run history */}
      <Card>
        <CardHeader>
          <CardTitle className="text-base">Run History</CardTitle>
        </CardHeader>
        <CardContent>
          <HandlerRunsTable runs={runs ?? []} handlers={handlers ?? []} />
        </CardContent>
      </Card>
    </div>
  );
};
