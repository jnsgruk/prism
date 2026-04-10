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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useTriggerBackfill } from "@/views/ingestion/hooks/use-ingestion";
import { Loader2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

export const BackfillDialog = ({
  sourceName,
  open,
  onOpenChange,
  onAction,
}: {
  sourceName: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onAction?: () => void;
}): React.ReactElement => {
  const backfill = useTriggerBackfill();
  const [sinceDate, setSinceDate] = useState("");

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!sinceDate) return;

    backfill.mutate(
      { sourceName, sinceDate },
      {
        onSuccess: () => {
          toast.success(`Backfill triggered for ${sourceName} since ${sinceDate}`);
          onOpenChange(false);
          setSinceDate("");
          onAction?.();
        },
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : "Failed to trigger backfill");
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>Backfill {sourceName}</DialogTitle>
            <DialogDescription>
              Re-ingest data from a specific date. This will fetch all data since the selected date, regardless of
              existing watermarks.
            </DialogDescription>
          </DialogHeader>
          <div className="mt-4 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="since-date">Since date</Label>
              <Input
                id="since-date"
                type="date"
                value={sinceDate}
                onChange={(e) => setSinceDate(e.target.value)}
                required
              />
            </div>
          </div>
          <DialogFooter className="mt-4">
            <DialogClose render={<Button type="button" variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={backfill.isPending || !sinceDate}>
              {backfill.isPending && <Loader2 className="mr-1 size-4 animate-spin" />}
              Start Backfill
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
