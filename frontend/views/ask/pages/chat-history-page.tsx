import { Check, MessageSquare, Paperclip, Pencil, Search, Trash2, X } from "lucide-react";
import { useCallback, useRef, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { toast } from "sonner";

import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import type { ConversationSummary } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

import {
  useDeleteConversation,
  useListConversations,
  useRenameConversation,
} from "@/lib/hooks/use-conversations";

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

const ConversationRow = ({
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

  return (
    <div className="group flex items-center gap-3 rounded-lg border px-4 py-3 transition-colors hover:bg-muted/50">
      <span
        className={`inline-block size-2 shrink-0 rounded-full ${statusDot(conv.containerStatus)}`}
      />
      <div className="min-w-0 flex-1">
        {isEditing ? (
          <div className="flex items-center gap-2">
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
        ) : (
          <Link to={`/ask/${conv.id}`} className="block">
            <span className="text-sm font-medium">{conv.title || "Untitled conversation"}</span>
            <div className="mt-0.5 flex items-center gap-3 text-xs text-muted-foreground">
              <span className="flex items-center gap-1">
                <MessageSquare className="size-3" />
                {conv.messageCount} messages
              </span>
              {conv.artifactCount > 0 && (
                <span className="flex items-center gap-1">
                  <Paperclip className="size-3" />
                  {conv.artifactCount} artifacts
                </span>
              )}
              <span>{formatRelative(conv.lastActivityAt)}</span>
            </div>
          </Link>
        )}
      </div>
      {!isEditing && (
        <div className="flex shrink-0 items-center gap-1 opacity-0 transition-opacity group-hover:opacity-100">
          <button
            type="button"
            className="rounded p-1.5 hover:bg-accent"
            onClick={() => onRenameStart(conv.id)}
          >
            <Pencil className="size-3.5 text-muted-foreground" />
          </button>
          <button
            type="button"
            className="rounded p-1.5 hover:bg-destructive/10"
            onClick={() => onDelete(conv.id)}
          >
            <Trash2 className="size-3.5 text-destructive" />
          </button>
        </div>
      )}
    </div>
  );
};

const ChatHistoryPage = (): React.ReactElement => {
  const navigate = useNavigate();
  const { data } = useListConversations(1, 50);
  const deleteMutation = useDeleteConversation();
  const renameMutation = useRenameConversation();

  const [search, setSearch] = useState("");
  const [editingId, setEditingId] = useState<string | null>(null);
  const [deleteAllDialogOpen, setDeleteAllDialogOpen] = useState(false);

  const searchTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const [debouncedSearch, setDebouncedSearch] = useState("");

  const handleSearchChange = (e: React.ChangeEvent<HTMLInputElement>): void => {
    const value = e.target.value;
    setSearch(value);
    clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => setDebouncedSearch(value), 300);
  };

  const conversations = (data?.conversations ?? []).filter(
    (c) =>
      !debouncedSearch || (c.title ?? "").toLowerCase().includes(debouncedSearch.toLowerCase()),
  );

  const handleDelete = useCallback(
    (id: string) => {
      deleteMutation.mutate(id, {
        onSuccess: () => toast.success("Conversation deleted"),
        onError: (err) =>
          toast.error(err instanceof Error ? err.message : "Failed to delete conversation"),
      });
    },
    [deleteMutation],
  );

  const handleDeleteAllConfirm = useCallback(() => {
    const ids = data?.conversations.map((c) => c.id) ?? [];
    if (ids.length === 0) return;
    for (const id of ids) {
      deleteMutation.mutate(id);
    }
    toast.success(`Deleting ${ids.length} conversation${ids.length > 1 ? "s" : ""}`);
    setDeleteAllDialogOpen(false);
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
          onError: (err) =>
            toast.error(err instanceof Error ? err.message : "Failed to rename conversation"),
        },
      );
    },
    [renameMutation],
  );

  return (
    <>
      <PageHeader title="Chat History" />
      <div className="min-w-0 flex-1 space-y-4 overflow-y-auto p-6">
        <div className="flex items-center gap-3">
          <div className="relative flex-1">
            <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder="Search conversations..."
              value={search}
              onChange={handleSearchChange}
              className="pl-8"
            />
          </div>
          {data && data.conversations.length > 0 && (
            <div className="flex items-center gap-2">
              <Badge variant="secondary">{data.totalCount} conversations</Badge>
              <Button
                variant="outline"
                size="sm"
                className="gap-1.5 text-destructive hover:bg-destructive/10 hover:text-destructive"
                onClick={() => setDeleteAllDialogOpen(true)}
              >
                <Trash2 className="size-3.5" />
                Delete all
              </Button>
            </div>
          )}
        </div>

        {conversations.length === 0 ? (
          <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
            <MessageSquare className="size-10 text-muted-foreground" />
            <p className="mb-1 font-medium">
              {debouncedSearch ? "No matching conversations" : "No conversations yet"}
            </p>
            <p className="text-sm text-muted-foreground">
              {debouncedSearch
                ? "Try a different search term"
                : "Start a conversation from the Ask page"}
            </p>
          </div>
        ) : (
          <div className="space-y-2">
            {conversations.map((conv) => (
              <ConversationRow
                key={conv.id}
                conv={conv}
                isEditing={editingId === conv.id}
                onDelete={handleDelete}
                onRenameStart={setEditingId}
                onRenameSubmit={handleRenameSubmit}
                onRenameCancel={() => setEditingId(null)}
              />
            ))}
          </div>
        )}
      </div>

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

export default ChatHistoryPage;
