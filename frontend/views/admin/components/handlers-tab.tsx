import { useCallback, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { AlertCircle, Cog, Loader2, Play, Square } from "lucide-react";
import { toast } from "sonner";

import type { HandlerInfo } from "@ps/api/gen/prism/v1/handlers_pb";

import { formatRelativeTime } from "@/lib/format";
import {
  useCancelHandlerRun,
  useListHandlers,
  useListRuns,
} from "@/views/ingestion/hooks/use-ingestion";

import { HandlerRunsTable } from "@/views/admin/components/handler-runs-table";
import { TriggerHandlerDialog } from "@/views/admin/components/trigger-handler-dialog";

/** Strip the "Handler" suffix for display. */
const displayName = (name: string): string => name.replace("Handler", "");

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

export const HandlersTab = (): React.ReactElement => {
  const { data: handlers, isLoading: handlersLoading, error: handlersError } = useListHandlers();
  const { data: runs } = useListRuns(undefined, { refetchInterval: 5000 });
  const cancelRun = useCancelHandlerRun();

  const handleCancel = useCallback(
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
                onCancelRun={handleCancel}
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
          <HandlerRunsTable
            runs={runs ?? []}
            handlers={handlers ?? []}
            onCancelRun={handleCancel}
            cancelPending={cancelRun.isPending}
          />
        </CardContent>
      </Card>
    </div>
  );
};
