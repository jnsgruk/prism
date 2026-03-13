import { useState } from "react";
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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { AlertCircle, Ban, CheckCircle2, Cog, Loader2, Play } from "lucide-react";
import { toast } from "sonner";

import type { HandlerInfo, IngestionRun } from "@ps/api/gen/prism/v1/ingestion_pb";
import { useListSources } from "@ps/hooks/use-config";

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
    date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
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

  const handleTrigger = (): void => {
    if (!sourceName || !method) return;
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
        </div>

        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button onClick={handleTrigger} disabled={!sourceName || !method || trigger.isPending}>
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

const RunDetailDialog = ({
  run,
  open,
  onOpenChange,
}: {
  run: IngestionRun;
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
          <DialogDescription>Source: {run.sourceName}</DialogDescription>
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
              <p className="text-xs text-muted-foreground">Items</p>
              <p className="font-medium">{run.itemsCollected.toLocaleString()}</p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Started</p>
              <p>
                {run.startedAt
                  ? new Date(Number(run.startedAt.seconds) * 1000).toLocaleString()
                  : "\u2014"}
              </p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Completed</p>
              <p>
                {run.completedAt
                  ? new Date(Number(run.completedAt.seconds) * 1000).toLocaleString()
                  : "\u2014"}
              </p>
            </div>
            <div>
              <p className="text-xs text-muted-foreground">Duration</p>
              <p>{formatDuration(run.startedAt, run.completedAt)}</p>
            </div>
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

const HandlerRunsTable = ({ runs }: { runs: IngestionRun[] }): React.ReactElement => {
  const [selectedRun, setSelectedRun] = useState<IngestionRun | null>(null);

  if (runs.length === 0) {
    return (
      <p className="py-8 text-center text-sm text-muted-foreground">
        No handler runs recorded yet.
      </p>
    );
  }

  return (
    <>
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Handler</TableHead>
            <TableHead>Method</TableHead>
            <TableHead>Source</TableHead>
            <TableHead>Started</TableHead>
            <TableHead>Duration</TableHead>
            <TableHead className="text-right">Items</TableHead>
            <TableHead>Status</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {runs.map((run) => {
            const runConfig = statusConfig[run.status] ?? defaultStatus;
            return (
              <TableRow key={run.id} className="cursor-pointer" onClick={() => setSelectedRun(run)}>
                <TableCell className="text-xs font-medium">{run.handlerName}</TableCell>
                <TableCell className="text-xs">{run.handlerMethod}</TableCell>
                <TableCell className="text-xs">{run.sourceName}</TableCell>
                <TableCell className="text-xs">{formatTimestamp(run.startedAt)}</TableCell>
                <TableCell className="text-xs">
                  {formatDuration(run.startedAt, run.completedAt)}
                </TableCell>
                <TableCell className="text-right text-xs">
                  {run.itemsCollected.toLocaleString()}
                </TableCell>
                <TableCell>
                  <Badge variant={runConfig.variant} className="gap-1">
                    {runConfig.icon}
                    {runConfig.label}
                  </Badge>
                </TableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>

      {selectedRun && (
        <RunDetailDialog
          run={selectedRun}
          open={!!selectedRun}
          onOpenChange={(open) => {
            if (!open) setSelectedRun(null);
          }}
        />
      )}
    </>
  );
};

export const HandlersTab = (): React.ReactElement => {
  const { data: handlers, isLoading: handlersLoading, error: handlersError } = useListHandlers();
  const { data: runs } = useListRuns(undefined, { refetchInterval: 5000 });
  const [handlerFilter, setHandlerFilter] = useState<string | undefined>(undefined);

  const filteredRuns = handlerFilter
    ? (runs?.filter((r) => r.handlerName === handlerFilter) ?? [])
    : (runs ?? []);

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
          <div className="flex items-center justify-between">
            <CardTitle className="text-base">Run History</CardTitle>
            <div className="flex gap-1">
              <Button
                variant={handlerFilter === undefined ? "default" : "outline"}
                size="sm"
                onClick={() => setHandlerFilter(undefined)}
              >
                All
              </Button>
              {handlers?.map((h) => (
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
          </div>
        </CardHeader>
        <CardContent>
          <HandlerRunsTable runs={filteredRuns} />
        </CardContent>
      </Card>
    </div>
  );
};
