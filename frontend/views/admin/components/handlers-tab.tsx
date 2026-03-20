import { useCallback, useMemo, useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { AlertCircle, ChevronDown, ChevronRight, History } from "lucide-react";
import { toast } from "sonner";

import type { HandlerRun } from "@ps/api/gen/prism/v1/handlers_pb";

import {
  useCancelHandlerRun,
  useListHandlers,
  useListRuns,
} from "@/views/ingestion/hooks/use-ingestion";

import { HandlerRunsTable } from "@/views/admin/components/handler-runs-table";
import { HandlerSection } from "@/views/admin/components/handler-section";

const useStatusCounts = (
  runs: HandlerRun[],
): { completed: number; failed: number; running: number } =>
  useMemo(() => {
    const counts = { completed: 0, failed: 0, running: 0 };
    for (const r of runs) {
      if (r.status === "completed" || r.status === "completed_with_warnings") counts.completed++;
      else if (r.status === "failed") counts.failed++;
      else if (r.status === "running") counts.running++;
    }
    return counts;
  }, [runs]);

export const HandlersTab = (): React.ReactElement => {
  const { data: handlers, isLoading: handlersLoading, error: handlersError } = useListHandlers();
  const { data: runs } = useListRuns(undefined, { refetchInterval: 5000 });
  const cancelRun = useCancelHandlerRun();
  const [historyOpen, setHistoryOpen] = useState(false);
  const statusCounts = useStatusCounts(runs ?? []);

  const handleCancel = useCallback(
    (runId: string) => {
      cancelRun.mutate(runId, {
        onSuccess: () => toast.success("Run cancelled"),
        onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel"),
      });
    },
    [cancelRun],
  );

  const { ingestionHandlers, systemHandlers } = useMemo(() => {
    if (!handlers) return { ingestionHandlers: [], systemHandlers: [] };
    const ingestionNames = new Set(["EnrichmentHandler", "EmbeddingHandler"]);
    return {
      ingestionHandlers: handlers.filter((h) => h.requiresKey || ingestionNames.has(h.name)),
      systemHandlers: handlers.filter((h) => !h.requiresKey && !ingestionNames.has(h.name)),
    };
  }, [handlers]);

  return (
    <div className="space-y-6 pt-4">
      {handlersLoading && <p className="text-sm text-muted-foreground">Loading handlers...</p>}

      {handlersError && (
        <Alert variant="destructive">
          <AlertCircle className="size-4" />
          Failed to load handlers.
        </Alert>
      )}

      {handlers && (
        <>
          {ingestionHandlers.length > 0 && (
            <HandlerSection
              title="Ingestion Handlers"
              description="Handlers that fetch data from external platforms."
              handlers={ingestionHandlers}
              onCancelRun={handleCancel}
              cancelPending={cancelRun.isPending}
            />
          )}

          {systemHandlers.length > 0 && (
            <HandlerSection
              title="System Handlers"
              description="Background tasks for metrics, identity, AI, and maintenance."
              handlers={systemHandlers}
              onCancelRun={handleCancel}
              cancelPending={cancelRun.isPending}
            />
          )}
        </>
      )}

      {/* Run history — collapsible, collapsed by default */}
      <Collapsible open={historyOpen} onOpenChange={setHistoryOpen}>
        <Card>
          <CardHeader className="cursor-pointer" onClick={() => setHistoryOpen(!historyOpen)}>
            <CollapsibleTrigger
              render={<button type="button" className="flex w-full items-center gap-2 text-left" />}
            >
              {historyOpen ? (
                <ChevronDown className="size-4" />
              ) : (
                <ChevronRight className="size-4" />
              )}
              <History className="size-4 text-muted-foreground" />
              <CardTitle className="text-base">Run History</CardTitle>
              {statusCounts.completed > 0 && (
                <Badge variant="secondary" className="ml-1">
                  {statusCounts.completed} completed
                </Badge>
              )}
              {statusCounts.failed > 0 && (
                <Badge variant="destructive" className="ml-1">
                  {statusCounts.failed} failed
                </Badge>
              )}
              {statusCounts.running > 0 && (
                <Badge variant="default" className="ml-1">
                  {statusCounts.running} running
                </Badge>
              )}
            </CollapsibleTrigger>
          </CardHeader>
          <CollapsibleContent>
            <CardContent className="pt-0">
              <HandlerRunsTable
                runs={runs ?? []}
                handlers={handlers ?? []}
                onCancelRun={handleCancel}
                cancelPending={cancelRun.isPending}
              />
            </CardContent>
          </CollapsibleContent>
        </Card>
      </Collapsible>
    </div>
  );
};
