import { Button } from "@/components/ui/button";
import { Loader2, Play, Square } from "lucide-react";
import { useCallback } from "react";
import { toast } from "sonner";

import { useCancelPipeline, useTriggerPipeline } from "@/views/ingestion/hooks/use-pipeline";

export const PipelineActions = ({
  pipelineId,
  isRunning,
  onAction,
}: {
  pipelineId?: string;
  isRunning: boolean;
  onAction: () => void;
}): React.ReactElement => {
  const trigger = useTriggerPipeline();
  const cancel = useCancelPipeline();

  const handleRun = useCallback(() => {
    trigger.mutate(undefined, {
      onSuccess: () => {
        toast.success("Pipeline started");
        onAction();
      },
      onError: (err) =>
        toast.error(err instanceof Error ? err.message : "Failed to start pipeline"),
    });
  }, [trigger, onAction]);

  const handleCancel = useCallback(() => {
    if (!pipelineId) return;
    cancel.mutate(pipelineId, {
      onSuccess: () => {
        toast.success("Pipeline cancellation requested");
        onAction();
      },
      onError: (err) =>
        toast.error(err instanceof Error ? err.message : "Failed to cancel pipeline"),
    });
  }, [cancel, pipelineId, onAction]);

  if (isRunning) {
    return (
      <Button
        variant="outline"
        size="sm"
        className="h-7 text-destructive hover:text-destructive"
        onClick={handleCancel}
        disabled={cancel.isPending}
      >
        {cancel.isPending ? (
          <Loader2 className="mr-1.5 size-3.5 animate-spin" />
        ) : (
          <Square className="mr-1.5 size-3.5" />
        )}
        Cancel
      </Button>
    );
  }

  return (
    <Button
      variant="outline"
      size="sm"
      className="h-7"
      onClick={handleRun}
      disabled={trigger.isPending}
    >
      {trigger.isPending ? (
        <Loader2 className="mr-1.5 size-3.5 animate-spin" />
      ) : (
        <Play className="mr-1.5 size-3.5" />
      )}
      Run Pipeline
    </Button>
  );
};
