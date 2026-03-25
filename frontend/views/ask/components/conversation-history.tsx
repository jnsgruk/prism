import { ChevronDown, MessageSquare, Paperclip, Pencil, Trash2 } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { toast } from "sonner";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import type { ConversationSummary } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

import {
  useDeleteConversation,
  useListConversations,
  useRenameConversation,
} from "@/views/ask/hooks/use-conversations";

const formatRelative = (ts?: { seconds: bigint }): string => {
  if (!ts) return "";
  const ms = Number(ts.seconds) * 1000;
  const diff = Date.now() - ms;
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
};

const statusDot = (containerStatus: string): string => {
  if (containerStatus === "active") return "bg-green-500";
  if (containerStatus === "error") return "bg-destructive";
  return "bg-muted-foreground";
};

const ConversationItemContent = ({
  conv,
  onDelete,
  onRename,
}: {
  conv: ConversationSummary;
  onDelete: (id: string) => void;
  onRename: (id: string, existingTitle: string) => void;
}): React.ReactElement => (
  <div className="flex w-full items-center gap-2">
    <div className="min-w-0 flex-1">
      <div className="flex items-center gap-1.5">
        <span
          className={`inline-block size-2 shrink-0 rounded-full ${statusDot(conv.containerStatus)}`}
        />
        <span className="truncate text-sm">{conv.title || "Untitled conversation"}</span>
      </div>
      <div className="mt-0.5 flex items-center gap-2 text-xs text-muted-foreground">
        <span className="flex items-center gap-0.5">
          <MessageSquare className="size-3" />
          {conv.messageCount}
        </span>
        {conv.artifactCount > 0 && (
          <span className="flex items-center gap-0.5">
            <Paperclip className="size-3" />
            {conv.artifactCount}
          </span>
        )}
        <span>{formatRelative(conv.lastActivityAt)}</span>
      </div>
    </div>
    <div className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-data-selected/command-item:opacity-100">
      <button
        type="button"
        className="rounded p-1 hover:bg-accent"
        onClick={(e) => {
          e.stopPropagation();
          onRename(conv.id, conv.title ?? "");
        }}
      >
        <Pencil className="size-3.5 text-muted-foreground" />
      </button>
      <button
        type="button"
        className="rounded p-1 hover:bg-destructive/10"
        onClick={(e) => {
          e.stopPropagation();
          onDelete(conv.id);
        }}
      >
        <Trash2 className="size-3.5 text-destructive" />
      </button>
    </div>
  </div>
);

export const ConversationHistory = (): React.ReactElement => {
  const { conversationId } = useParams<{ conversationId?: string }>();
  const navigate = useNavigate();
  const { data } = useListConversations(1, 50);
  const deleteMutation = useDeleteConversation();
  const renameMutation = useRenameConversation();

  const [open, setOpen] = useState(false);
  const [renameDialogOpen, setRenameDialogOpen] = useState(false);
  const [renameTarget, setRenameTarget] = useState<{ id: string; title: string } | null>(null);
  const renameInputRef = useRef<HTMLInputElement>(null);

  const currentTitle =
    data?.conversations.find((c) => c.id === conversationId)?.title ?? "New conversation";

  const handleSelect = useCallback(
    (id: string) => {
      setOpen(false);
      navigate(`/ask/${id}`);
    },
    [navigate],
  );

  const handleDelete = useCallback(
    (id: string) => {
      deleteMutation.mutate(id, {
        onSuccess: () => {
          toast.success("Conversation deleted");
          if (id === conversationId) {
            navigate("/ask");
          }
        },
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : "Failed to delete conversation");
        },
      });
    },
    [deleteMutation, conversationId, navigate],
  );

  const handleDeleteAll = useCallback(() => {
    const ids = data?.conversations.map((c) => c.id) ?? [];
    if (ids.length === 0) return;
    for (const id of ids) {
      deleteMutation.mutate(id);
    }
    toast.success(`Deleting ${ids.length} conversation${ids.length > 1 ? "s" : ""}`);
    navigate("/ask");
  }, [data, deleteMutation, navigate]);

  const handleRenameStart = useCallback((id: string, existingTitle: string) => {
    setRenameTarget({ id, title: existingTitle });
    setRenameDialogOpen(true);
  }, []);

  const handleRenameSubmit = useCallback(
    (e: React.FormEvent) => {
      e.preventDefault();
      if (!renameTarget) return;
      const title = renameInputRef.current?.value.trim();
      if (!title) return;

      renameMutation.mutate(
        { conversationId: renameTarget.id, title },
        {
          onSuccess: () => {
            toast.success("Conversation renamed");
            setRenameDialogOpen(false);
            setRenameTarget(null);
          },
          onError: (err) => {
            toast.error(err instanceof Error ? err.message : "Failed to rename conversation");
          },
        },
      );
    },
    [renameTarget, renameMutation],
  );

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger
          render={
            <Button variant="outline" className="h-9 w-80 justify-between gap-2 px-3 text-sm" />
          }
        >
          <span className="truncate text-sm">
            {conversationId ? currentTitle : "Select conversation"}
          </span>
          <div className="flex items-center gap-1">
            {data && data.totalCount > 0 && <Badge variant="secondary">{data.totalCount}</Badge>}
            <ChevronDown className="size-3.5 text-muted-foreground" />
          </div>
        </PopoverTrigger>
        <PopoverContent className="w-[36rem] p-0" align="center">
          <Command>
            <div className="flex items-center gap-1 pr-1">
              <CommandInput placeholder="Search conversations..." />
              {data && data.conversations.length > 0 && (
                <button
                  type="button"
                  className="shrink-0 rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
                  title="Delete all conversations"
                  onClick={handleDeleteAll}
                >
                  <Trash2 className="size-4" />
                </button>
              )}
            </div>
            <CommandList className="max-h-[28rem] p-1">
              <CommandEmpty>No conversations found.</CommandEmpty>
              {data?.conversations.map((conv) => (
                <CommandItem
                  key={conv.id}
                  value={`${conv.title ?? "Untitled conversation"} ${conv.id}`}
                  onSelect={() => handleSelect(conv.id)}
                  className="group/command-item py-2.5"
                >
                  <ConversationItemContent
                    conv={conv}
                    onDelete={handleDelete}
                    onRename={handleRenameStart}
                  />
                </CommandItem>
              ))}
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>

      <Dialog open={renameDialogOpen} onOpenChange={setRenameDialogOpen}>
        <DialogContent>
          <form onSubmit={handleRenameSubmit}>
            <DialogHeader>
              <DialogTitle>Rename conversation</DialogTitle>
            </DialogHeader>
            <div className="space-y-2 py-4">
              <Label htmlFor="conv-title">Title</Label>
              <Input
                id="conv-title"
                ref={renameInputRef}
                defaultValue={renameTarget?.title ?? ""}
                maxLength={200}
                required
                autoFocus
              />
            </div>
            <DialogFooter>
              <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
              <Button type="submit" disabled={renameMutation.isPending}>
                {renameMutation.isPending ? "Saving..." : "Save"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </>
  );
};
