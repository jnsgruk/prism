import { useCallback, useMemo } from "react";
import { Alert } from "@/components/ui/alert";
import { AlertCircle } from "lucide-react";
import { toast } from "sonner";

import {
  useCancelHandlerRun,
  useListHandlers,
  useListRuns,
} from "@/views/ingestion/hooks/use-ingestion";

import { HandlerRunsCard } from "@/views/admin/components/handler-runs-table";
import { HandlerSection } from "@/views/admin/components/handler-section";

export const HandlersTab = (): React.ReactElement => {
  const {
    data: handlers,
    isLoading: handlersLoading,
    error: handlersError,
  } = useListHandlers({ refetchInterval: 2_000 });
  const { data: runs } = useListRuns(undefined, { refetchInterval: 2_000 });
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

      <HandlerRunsCard
        runs={runs ?? []}
        handlers={handlers ?? []}
        onCancelRun={handleCancel}
        cancelPending={cancelRun.isPending}
      />
    </div>
  );
};
