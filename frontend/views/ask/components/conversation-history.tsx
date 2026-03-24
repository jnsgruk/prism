import { History, MessageSquare, Paperclip } from "lucide-react";
import { Link } from "react-router-dom";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger } from "@/components/ui/sheet";
import type { ConversationSummary } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

import { useListConversations } from "@/views/ask/hooks/use-conversations";

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

const ConversationItem = ({ conv }: { conv: ConversationSummary }): React.ReactElement => (
  <Link
    to={`/ask/${conv.id}`}
    className="flex items-start gap-3 rounded-md px-3 py-2.5 transition-colors hover:bg-muted"
  >
    <div className="min-w-0 flex-1">
      <div className="flex items-center gap-1.5">
        <span className={`inline-block size-2 rounded-full ${statusDot(conv.containerStatus)}`} />
        <p className="truncate text-sm font-medium">{conv.title || "Untitled conversation"}</p>
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
  </Link>
);

export const ConversationHistory = (): React.ReactElement => {
  const { data } = useListConversations();

  return (
    <Sheet>
      <SheetTrigger render={<Button variant="outline" size="sm" className="gap-1.5" />}>
        <History className="size-4" />
        History
        {data && data.totalCount > 0 && (
          <Badge variant="secondary" className="ml-0.5">
            {data.totalCount}
          </Badge>
        )}
      </SheetTrigger>
      <SheetContent className="w-80 sm:w-96">
        <SheetHeader>
          <SheetTitle>Conversations</SheetTitle>
        </SheetHeader>
        <div className="mt-4 max-h-[calc(100vh-8rem)] space-y-1 overflow-y-auto">
          {data?.conversations.length === 0 && (
            <p className="py-8 text-center text-sm text-muted-foreground">No conversations yet.</p>
          )}
          {data?.conversations.map((conv) => (
            <ConversationItem key={conv.id} conv={conv} />
          ))}
        </div>
      </SheetContent>
    </Sheet>
  );
};
