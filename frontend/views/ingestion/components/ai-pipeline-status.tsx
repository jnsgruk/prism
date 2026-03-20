import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { Brain, Loader2, Play, Sparkles, Square } from "lucide-react";
import { toast } from "sonner";

import { useEnrichmentPipelineStatus } from "@/views/admin/hooks/use-enrichment";
import { useEmbeddingStatus } from "@/lib/hooks/use-embeddings";
import {
  useCancelHandlerRun,
  useListRuns,
  useTriggerHandler,
} from "@/views/ingestion/hooks/use-ingestion";

const ENRICHMENT_TYPE_LABELS: Record<string, string> = {
  review_depth: "Review Depth",
  sentiment: "Sentiment",
  significance: "Significance",
  topic: "Topic",
};

const EmbeddingSection = (): React.ReactElement => {
  const { data: embStatus } = useEmbeddingStatus();
  const triggerHandler = useTriggerHandler();
  const cancelRun = useCancelHandlerRun();

  const { data: embRuns } = useListRuns(undefined, {
    refetchInterval: 5_000,
    handlerName: "EmbeddingHandler",
  });
  const activeEmbRun = embRuns?.find((r) => r.status === "running");
  const isEmbRunning = !!activeEmbRun;

  const handleTriggerEmb = (): void => {
    triggerHandler.mutate(
      { handlerName: "EmbeddingHandler", method: "run_cycle", key: "" },
      {
        onSuccess: () => toast.success("Embedding run started"),
        onError: (err) =>
          toast.error(err instanceof Error ? err.message : "Failed to trigger embedding"),
      },
    );
  };

  const handleCancelEmb = (): void => {
    if (!activeEmbRun) return;
    cancelRun.mutate(activeEmbRun.id, {
      onSuccess: () => toast.success("Embedding run cancelled"),
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel"),
    });
  };

  if (!embStatus) return <></>;

  const lastEmbLabel = embStatus.lastEmbeddedAt
    ? formatRelativeTime(embStatus.lastEmbeddedAt)
    : "Never";

  return (
    <>
      <Separator />
      <div className="space-y-3">
        <div className="flex items-center justify-between">
          <p className="text-xs font-medium text-muted-foreground">Embeddings</p>
          {isEmbRunning ? (
            <Button
              variant="destructive"
              size="sm"
              onClick={handleCancelEmb}
              disabled={cancelRun.isPending}
            >
              <Square className="mr-1.5 size-3.5" />
              Cancel
            </Button>
          ) : (
            <Button
              variant="outline"
              size="sm"
              onClick={handleTriggerEmb}
              disabled={triggerHandler.isPending}
            >
              <Play className="mr-1.5 size-3.5" />
              Run Embedding
            </Button>
          )}
        </div>
        <div className="grid grid-cols-4 gap-4">
          <div>
            <p className="text-xs text-muted-foreground">Queued</p>
            <p className="text-lg font-semibold tabular-nums">{embStatus.queuedCount.toString()}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">Embedded</p>
            <p className="text-lg font-semibold tabular-nums">
              {embStatus.embeddedCount.toString()}
            </p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">Coverage</p>
            <p className="text-lg font-semibold tabular-nums">
              {Math.round(embStatus.coveragePercent)}%
            </p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">Last Run</p>
            <p className="text-sm font-medium">{lastEmbLabel}</p>
          </div>
        </div>
        {/* Coverage progress bar */}
        <div className="h-2 overflow-hidden rounded-full bg-muted">
          <div
            className="h-full rounded-full bg-primary transition-all"
            style={{ width: `${Math.min(embStatus.coveragePercent, 100)}%` }}
          />
        </div>
      </div>
    </>
  );
};

const AiPipelineStatus = (): React.ReactElement => {
  const { data: status, isLoading } = useEnrichmentPipelineStatus();
  const triggerHandler = useTriggerHandler();
  const cancelRun = useCancelHandlerRun();

  // Check if there's an active enrichment run
  const { data: runs } = useListRuns(undefined, {
    refetchInterval: 5_000,
    handlerName: "EnrichmentHandler",
  });
  const activeRun = runs?.find((r) => r.status === "running");
  const isRunning = !!activeRun;

  const handleTrigger = (): void => {
    triggerHandler.mutate(
      { handlerName: "EnrichmentHandler", method: "run_cycle", key: "" },
      {
        onSuccess: () => {
          toast.success("Enrichment run started");
        },
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : "Failed to trigger enrichment");
        },
      },
    );
  };

  const handleCancel = (): void => {
    if (!activeRun) return;
    cancelRun.mutate(activeRun.id, {
      onSuccess: () => {
        toast.success("Enrichment run cancelled");
      },
      onError: (err) => {
        toast.error(err instanceof Error ? err.message : "Failed to cancel");
      },
    });
  };

  if (isLoading) {
    return (
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="flex items-center gap-2 text-sm font-semibold">
            <Brain className="size-4" />
            AI Pipeline
          </CardTitle>
        </CardHeader>
        <CardContent className="space-y-3">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </CardContent>
      </Card>
    );
  }

  if (!status) return <></>;

  const lastRunLabel = status.lastEnrichmentAt
    ? formatRelativeTime(status.lastEnrichmentAt)
    : "Never";

  return (
    <Card>
      <CardHeader className="pb-3">
        <div className="flex items-center justify-between">
          <CardTitle className="flex items-center gap-2 text-sm font-semibold">
            <Brain className="size-4" />
            AI Pipeline
          </CardTitle>
          {isRunning ? (
            <Button
              variant="destructive"
              size="sm"
              onClick={handleCancel}
              disabled={cancelRun.isPending}
            >
              {cancelRun.isPending ? (
                <Loader2 className="mr-1.5 size-3.5 animate-spin" />
              ) : (
                <Square className="mr-1.5 size-3.5" />
              )}
              Cancel
            </Button>
          ) : (
            <Button
              variant="outline"
              size="sm"
              onClick={handleTrigger}
              disabled={triggerHandler.isPending}
            >
              {triggerHandler.isPending ? (
                <Loader2 className="mr-1.5 size-3.5 animate-spin" />
              ) : (
                <Play className="mr-1.5 size-3.5" />
              )}
              Run Enrichment
            </Button>
          )}
        </div>
      </CardHeader>
      <CardContent className="space-y-4">
        {/* Summary stats */}
        <div className={`grid gap-4 ${isRunning ? "grid-cols-4" : "grid-cols-3"}`}>
          <div>
            <p className="text-xs text-muted-foreground">Pending</p>
            <p className="text-lg font-semibold tabular-nums">{status.pendingCount.toString()}</p>
          </div>
          <div>
            <p className="text-xs text-muted-foreground">Total Enrichments</p>
            <p className="text-lg font-semibold tabular-nums">
              {status.totalEnrichments.toString()}
            </p>
          </div>
          {isRunning && activeRun && (
            <div>
              <p className="text-xs text-muted-foreground">Enriched This Run</p>
              <p className="text-lg font-semibold tabular-nums">
                {activeRun.itemsCollected.toLocaleString()}
              </p>
            </div>
          )}
          <div>
            <p className="text-xs text-muted-foreground">Last Run</p>
            <p className="text-sm font-medium">{lastRunLabel}</p>
          </div>
        </div>

        {/* By-type breakdown */}
        {status.byType.length > 0 && (
          <>
            <Separator />
            <div className="space-y-2">
              <p className="text-xs font-medium text-muted-foreground">Enrichments by Type</p>
              <div className="flex flex-wrap gap-2">
                {status.byType.map((t) => (
                  <Badge key={t.enrichmentType} variant="secondary" className="gap-1">
                    <Sparkles className="size-3" />
                    {ENRICHMENT_TYPE_LABELS[t.enrichmentType] ?? t.enrichmentType}
                    <span className="ml-0.5 tabular-nums">{t.count.toString()}</span>
                  </Badge>
                ))}
              </div>
            </div>
          </>
        )}

        {/* Embedding pipeline section */}
        <EmbeddingSection />
      </CardContent>
    </Card>
  );
};

const formatRelativeTime = (isoString: string): string => {
  const date = new Date(isoString);
  const now = Date.now();
  const diffMs = now - date.getTime();
  const diffMins = Math.floor(diffMs / 60_000);

  if (diffMins < 1) return "Just now";
  if (diffMins < 60) return `${diffMins}m ago`;
  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) return `${diffHours}h ago`;
  const diffDays = Math.floor(diffHours / 24);
  return `${diffDays}d ago`;
};

export { AiPipelineStatus };
