import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useSaveInsightFromConversation } from "@/lib/hooks/use-conversations";
import { Lightbulb, Loader2 } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

export const SaveInsightDialog = ({
  conversationId,
  messageId,
}: {
  conversationId: string;
  messageId: string;
}): React.ReactElement => {
  const [open, setOpen] = useState(false);
  const [title, setTitle] = useState("");
  const save = useSaveInsightFromConversation();

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    const trimmed = title.trim();
    if (!trimmed) return;

    save.mutate(
      { conversationId, messageId, title: trimmed },
      {
        onSuccess: () => {
          toast.success("Saved as insight");
          setOpen(false);
          setTitle("");
        },
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : "Failed to save insight");
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button variant="ghost" size="sm" className="gap-1.5 text-xs" />}>
        <Lightbulb className="size-3.5" />
        Save as Insight
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>Save as Insight</DialogTitle>
            <DialogDescription>Save this answer as a persistent insight snapshot.</DialogDescription>
          </DialogHeader>
          <div className="space-y-2 py-4">
            <Label htmlFor="insight-title">Title</Label>
            <Input
              id="insight-title"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              placeholder="e.g. Tox to UV migration status"
              required
            />
          </div>
          <DialogFooter>
            <Button variant="outline" type="button" onClick={() => setOpen(false)}>
              Cancel
            </Button>
            <Button type="submit" disabled={save.isPending || !title.trim()}>
              {save.isPending && <Loader2 className="mr-1.5 size-3.5 animate-spin" />}
              {save.isPending ? "Saving..." : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
