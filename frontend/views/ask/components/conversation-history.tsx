import { Check, ChevronDown, MessageSquare, Paperclip, Pencil, Trash2, X } from "lucide-react";
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
  isEditing,
  onDelete,
  onRenameStart,
  onRenameSubmit,
  onRenameCancel,
}: {
  conv: ConversationSummary;
  isEditing: boolean;
  onDelete: (id: string) => void;
  onRenameStart: (id: string) => void;
  onRenameSubmit: (id: string, title: string) => void;
  onRenameCancel: () => void;
}): React.ReactElement => {
  const inputRef = useRef<HTMLInputElement>(null);

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>): void => {
    if (e.key === "Enter") {
      e.preventDefault();
      const title = inputRef.current?.value.trim();
      if (title) onRenameSubmit(conv.id, title);
    } else if (e.key === "Escape") {
      e.preventDefault();
      onRenameCancel();
    }
  };

  if (isEditing) {
    return (
      <div
        className="flex w-full items-center gap-2"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        <span
          className={`inline-block size-2 shrink-0 rounded-full ${statusDot(conv.containerStatus)}`}
        />
        <input
          ref={inputRef}
          type="text"
          defaultValue={conv.title ?? ""}
          maxLength={200}
          autoFocus
          onKeyDown={handleKeyDown}
          className="min-w-0 flex-1 rounded border border-input bg-background px-2 py-0.5 text-sm outline-none focus:ring-1 focus:ring-ring"
        />
        <button
          type="button"
          className="shrink-0 rounded p-1 hover:bg-accent"
          onClick={() => {
            const title = inputRef.current?.value.trim();
            if (title) onRenameSubmit(conv.id, title);
          }}
        >
          <Check className="size-3.5 text-green-600" />
        </button>
        <button
          type="button"
          className="shrink-0 rounded p-1 hover:bg-accent"
          onClick={onRenameCancel}
        >
          <X className="size-3.5 text-muted-foreground" />
        </button>
      </div>
    );
  }

  return (
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
            onRenameStart(conv.id);
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
};

export const ConversationHistory = (): React.ReactElement => {
  const { conversationId } = useParams<{ conversationId?: string }>();
  const navigate = useNavigate();
  const { data } = useListConversations(1, 50);
  const deleteMutation = useDeleteConversation();
  const renameMutation = useRenameConversation();

  const [open, setOpen] = useState(false);
  const [deleteAllDialogOpen, setDeleteAllDialogOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);

  const currentTitle =
    data?.conversations.find((c) => c.id === conversationId)?.title ?? "New conversation";

  const handleSelect = useCallback(
    (id: string) => {
      if (editingId) return;
      setOpen(false);
      navigate(`/ask/${id}`);
    },
    [navigate, editingId],
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

  const handleDeleteAllConfirm = useCallback(() => {
    const ids = data?.conversations.map((c) => c.id) ?? [];
    if (ids.length === 0) return;
    for (const id of ids) {
      deleteMutation.mutate(id);
    }
    toast.success(`Deleting ${ids.length} conversation${ids.length > 1 ? "s" : ""}`);
    setDeleteAllDialogOpen(false);
    setOpen(false);
    navigate("/ask");
  }, [data, deleteMutation, navigate]);

  const handleRenameSubmit = useCallback(
    (id: string, title: string) => {
      renameMutation.mutate(
        { conversationId: id, title },
        {
          onSuccess: () => {
            toast.success("Conversation renamed");
            setEditingId(null);
          },
          onError: (err) => {
            toast.error(err instanceof Error ? err.message : "Failed to rename conversation");
          },
        },
      );
    },
    [renameMutation],
  );

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger
          render={
            <Button variant="outline" className="h-10 w-96 justify-between gap-2 px-4 text-sm" />
          }
        >
          <span className="truncate text-sm">
            {conversationId ? currentTitle : "Select conversation"}
          </span>
          <div className="flex items-center gap-1">
            {data && data.totalCount > 0 && <Badge variant="secondary">{data.totalCount}</Badge>}
            <ChevronDown className="size-4 text-muted-foreground" />
          </div>
        </PopoverTrigger>
        <PopoverContent className="w-[36rem] p-0" align="center">
          <Command>
            <div className="flex items-center gap-1 pr-1 *:first:min-w-0 *:first:flex-1">
              <CommandInput placeholder="Search conversations..." />
              {data && data.conversations.length > 0 && (
                <button
                  type="button"
                  className="shrink-0 rounded-md p-1.5 text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
                  title="Delete all conversations"
                  onClick={() => setDeleteAllDialogOpen(true)}
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
                    isEditing={editingId === conv.id}
                    onDelete={handleDelete}
                    onRenameStart={setEditingId}
                    onRenameSubmit={handleRenameSubmit}
                    onRenameCancel={() => setEditingId(null)}
                  />
                </CommandItem>
              ))}
            </CommandList>
          </Command>
        </PopoverContent>
      </Popover>

      <Dialog open={deleteAllDialogOpen} onOpenChange={setDeleteAllDialogOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Delete all conversations?</DialogTitle>
          </DialogHeader>
          <p className="text-sm text-muted-foreground">
            This will permanently delete {data?.totalCount ?? 0} conversation
            {(data?.totalCount ?? 0) !== 1 ? "s" : ""} and reap their containers. This action cannot
            be undone.
          </p>
          <DialogFooter>
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button variant="destructive" onClick={handleDeleteAllConfirm}>
              Delete all
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </>
  );
};
