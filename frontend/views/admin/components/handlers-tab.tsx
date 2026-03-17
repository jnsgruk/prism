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
import { Separator } from "@/components/ui/separator";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import type { ColumnDef } from "@tanstack/react-table";
import { AlertCircle, Cog, Loader2, Play, Square } from "lucide-react";
import { toast } from "sonner";

import type { HandlerInfo, HandlerRun } from "@ps/api/gen/prism/v1/handlers_pb";
import { useListSources } from "@ps/hooks/use-config";

import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { RunDetailDialog } from "@/components/run-detail-dialog";
import { formatDuration, formatRelativeTime, formatTimestamp } from "@/lib/format";
import { defaultStatus, statusConfig } from "@/lib/run-utils";
import type { StatusFilter } from "@/lib/run-utils";
import {
  useCancelHandlerRun,
  useListHandlers,
  useListRuns,
  useTriggerHandler,
} from "@/views/ingestion/hooks/use-ingestion";

/** Strip the "Handler" suffix for display. */
const displayName = (name: string): string => name.replace("Handler", "");

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
            `Triggered ${displayName(handler.name)}.${method} (${resp.invocationId.slice(0, 12)}...)`,
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
          <DialogTitle>Trigger {displayName(handler.name)}</DialogTitle>
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

// ---------------------------------------------------------------------------
// Handler card — mirrors SourceStatusRow pattern from the ingestion page
// ---------------------------------------------------------------------------

const HandlerCard = ({
  handler,
  onCancelRun,
  cancelPending,
}: {
  handler: HandlerInfo;
  onCancelRun: (runId: string) => void;
  cancelPending: boolean;
}): React.ReactElement => {
  const [triggerOpen, setTriggerOpen] = useState(false);
  const isRunning = !!handler.activeRun;

  return (
    <>
      <div className="rounded-lg border bg-card">
        <div className="flex items-start gap-6 p-5">
          {/* Left: identity */}
          <div className="min-w-0 flex-1">
            <div className="flex items-center gap-3">
              <Cog className="size-5 shrink-0 text-muted-foreground" />
              <div>
                <div className="flex items-center gap-2">
                  <p className="text-sm font-semibold">{displayName(handler.name)}</p>
                  {isRunning && (
                    <Badge variant="default" className="gap-1 text-xs">
                      <Loader2 className="size-3 animate-spin" />
                      Running
                    </Badge>
                  )}
                </div>
                <p className="mt-0.5 text-xs text-muted-foreground">{handler.description}</p>
                <div className="mt-1.5 flex gap-1">
                  {handler.methods.map((m) => (
                    <Badge key={m} variant="outline" className="text-xs">
                      {m}
                    </Badge>
                  ))}
                </div>
              </div>
            </div>
          </div>

          {/* Right: actions */}
          <div className="flex shrink-0 items-center gap-2">
            {isRunning ? (
              <Button
                variant="destructive"
                size="sm"
                disabled={cancelPending}
                onClick={() => {
                  if (handler.activeRun) onCancelRun(handler.activeRun.runId);
                }}
              >
                {cancelPending ? (
                  <Loader2 className="mr-1.5 size-3.5 animate-spin" />
                ) : (
                  <Square className="mr-1.5 size-3.5" />
                )}
                Cancel
              </Button>
            ) : (
              <Button variant="outline" size="sm" onClick={() => setTriggerOpen(true)}>
                <Play className="mr-1.5 size-3.5" />
                Run
              </Button>
            )}
          </div>
        </div>

        {/* Active run info */}
        {handler.activeRun && (
          <>
            <Separator />
            <div className="flex gap-6 px-5 py-3 text-xs text-muted-foreground">
              <span>
                Method:{" "}
                <span className="font-medium text-foreground">{handler.activeRun.method}</span>
              </span>
              {handler.activeRun.key && (
                <span>
                  Source:{" "}
                  <span className="font-medium text-foreground">{handler.activeRun.key}</span>
                </span>
              )}
              {handler.activeRun.startedAt && (
                <span>Started {formatRelativeTime(handler.activeRun.startedAt)}</span>
              )}
            </div>
          </>
        )}
      </div>
      <TriggerHandlerDialog handler={handler} open={triggerOpen} onOpenChange={setTriggerOpen} />
    </>
  );
};

// ---------------------------------------------------------------------------
// Run history table columns — handler-centric, never shows "_system"
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Run history table
// ---------------------------------------------------------------------------

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
  const [pageSize, setPageSize] = useState(10);
  const [pageIndex, setPageIndex] = useState(0);
  const cancelRun = useCancelHandlerRun();

  const handleCancel = useCallback(
    (runId: string) => {
      cancelRun.mutate(runId, {
        onSuccess: () => {
          toast.success("Run cancelled");
          setSelectedRun(null);
        },
        onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel"),
      });
    },
    [cancelRun],
  );

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
              {displayName(h.name)}
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
          title={`${displayName(selectedRun.handlerName)}.${selectedRun.handlerMethod}`}
          description={
            selectedRun.sourceName === "_system"
              ? "Run details"
              : `Source: ${selectedRun.sourceName}`
          }
          open={!!selectedRun}
          onOpenChange={(open) => {
            if (!open) setSelectedRun(null);
          }}
          onCancel={handleCancel}
          cancelPending={cancelRun.isPending}
        />
      )}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Main tab
// ---------------------------------------------------------------------------

export const HandlersTab = (): React.ReactElement => {
  const { data: handlers, isLoading: handlersLoading, error: handlersError } = useListHandlers();
  const { data: runs } = useListRuns(undefined, { refetchInterval: 5000 });
  const cancelRun = useCancelHandlerRun();

  const handleCancelFromCard = useCallback(
    (runId: string) => {
      cancelRun.mutate(runId, {
        onSuccess: () => toast.success("Run cancelled"),
        onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel"),
      });
    },
    [cancelRun],
  );

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
          <div className="space-y-3">
            {handlers.map((h) => (
              <HandlerCard
                key={h.name}
                handler={h}
                onCancelRun={handleCancelFromCard}
                cancelPending={cancelRun.isPending}
              />
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
