import { Button } from "@/components/ui/button";
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
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useCancelPipeline, useTriggerPipeline } from "@/views/ingestion/hooks/use-pipeline";
import { Loader2, MoreVertical, Play, RotateCcw, Square } from "lucide-react";
import { useCallback, useState } from "react";
import { toast } from "sonner";

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
  const [showBackfill, setShowBackfill] = useState(false);
  const [sinceDate, setSinceDate] = useState("");

  const handleRun = useCallback(() => {
    trigger.mutate(undefined, {
      onSuccess: () => {
        toast.success("Pipeline started");
        onAction();
      },
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to start pipeline"),
    });
  }, [trigger, onAction]);

  const handleBackfill = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault();
      if (!sinceDate) return;

      trigger.mutate(
        { sinceDate },
        {
          onSuccess: () => {
            toast.success(`Backfill pipeline started from ${sinceDate}`);
            setShowBackfill(false);
            setSinceDate("");
            onAction();
          },
          onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to start backfill"),
        },
      );
    },
    [trigger, sinceDate, onAction],
  );

  const handleCancel = useCallback(() => {
    if (!pipelineId) return;
    cancel.mutate(pipelineId, {
      onSuccess: () => {
        toast.success("Pipeline cancellation requested");
        onAction();
      },
      onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to cancel pipeline"),
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
    <>
      <div className="flex items-center">
        <Button
          variant="outline"
          size="sm"
          className="h-7 rounded-r-none border-r-0"
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
        <DropdownMenu>
          <DropdownMenuTrigger
            render={
              <Button variant="outline" size="sm" className="h-7 rounded-l-none px-1.5" disabled={trigger.isPending} />
            }
          >
            <MoreVertical className="size-3.5" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={() => setShowBackfill(true)}>
              <RotateCcw className="size-3.5" />
              Backfill...
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <Dialog open={showBackfill} onOpenChange={setShowBackfill}>
        <DialogContent>
          <form onSubmit={handleBackfill}>
            <DialogHeader>
              <DialogTitle>Run Backfill Pipeline</DialogTitle>
              <DialogDescription>
                Re-ingest all enabled sources from a specific date, then run the full processing pipeline (metrics,
                enrichment, embedding, insights).
              </DialogDescription>
            </DialogHeader>
            <div className="mt-4 space-y-4">
              <div className="space-y-2">
                <Label htmlFor="backfill-since-date">Since date</Label>
                <Input
                  id="backfill-since-date"
                  type="date"
                  value={sinceDate}
                  onChange={(e) => setSinceDate(e.target.value)}
                  required
                />
              </div>
            </div>
            <DialogFooter className="mt-4">
              <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
              <Button type="submit" disabled={trigger.isPending || !sinceDate}>
                {trigger.isPending && <Loader2 className="mr-1 size-4 animate-spin" />}
                Start Backfill
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
};
