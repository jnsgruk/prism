import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useResetData } from "@/views/admin/hooks/use-admin";
import { RotateCcw } from "lucide-react";
import { useState } from "react";

export const ResetDataDialog = (): React.ReactElement => {
  const resetData = useResetData();
  const [confirmation, setConfirmation] = useState("");
  const [open, setOpen] = useState(false);

  const confirmed = confirmation === "RESET";

  const handleReset = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!confirmed) return;
    resetData.mutate(undefined, {
      onSuccess: () => {
        setOpen(false);
        setConfirmation("");
      },
    });
  };

  const handleOpenChange = (next: boolean): void => {
    setOpen(next);
    if (!next) {
      setConfirmation("");
      resetData.reset();
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger render={<Button variant="destructive" />}>
        <RotateCcw className="size-4" />
        Reset Data
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={handleReset}>
          <DialogHeader>
            <DialogTitle>Reset All Data</DialogTitle>
            <DialogDescription>
              This will permanently delete all ingested contributions, imported teams, people, platform identities, and
              metric snapshots. Source configurations will be preserved.
            </DialogDescription>
          </DialogHeader>

          <div className="mt-4 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="reset-confirm">
                Type <span className="font-mono font-bold">RESET</span> to confirm
              </Label>
              <Input
                id="reset-confirm"
                value={confirmation}
                onChange={(e) => setConfirmation(e.target.value)}
                placeholder="RESET"
                autoComplete="off"
              />
            </div>

            {resetData.isSuccess && (
              <Alert>
                Deleted {resetData.data.contributionsDeleted} contributions, {resetData.data.peopleDeleted} people,{" "}
                {resetData.data.teamsDeleted} teams.
              </Alert>
            )}

            {resetData.isError && (
              <Alert variant="destructive">
                {resetData.error instanceof Error ? resetData.error.message : "Reset failed"}
              </Alert>
            )}
          </div>

          <DialogFooter className="mt-4">
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" variant="destructive" disabled={!confirmed || resetData.isPending}>
              {resetData.isPending ? "Resetting..." : "Reset All Data"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
