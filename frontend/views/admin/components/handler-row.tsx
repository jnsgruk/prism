import { DOT_SEP, Stat } from "@/components/inline-stat";
import { CancelButton } from "@/components/run-cancel-buttons";
import { StatusDot } from "@/components/status-dot";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { ChevronRight, Cog } from "lucide-react";
import { useState } from "react";

import type { HandlerInfo } from "@ps/api/gen/prism/v1/handlers_pb";
import { cn } from "@ps/cn";

import { formatRelativeTime } from "@/lib/format";
import { TriggerHandlerPopover } from "./trigger-handler-popover";

/** Strip the "Handler" suffix for display. */
const displayName = (name: string): string => name.replace("Handler", "");

export const HandlerRow = ({
  handler,
  onCancelRun,
  cancelPending,
}: {
  handler: HandlerInfo;
  onCancelRun: (runId: string) => void;
  cancelPending: boolean;
}): React.ReactElement => {
  const [expanded, setExpanded] = useState(false);
  const isRunning = !!handler.activeRun;
  const hasDetail = isRunning;

  return (
    <Collapsible open={expanded} onOpenChange={setExpanded}>
      <div
        className={cn(
          "group grid items-center gap-x-3 border-b px-4 py-2.5 text-sm last:border-b-0",
          "grid-cols-[1rem_minmax(7rem,1fr)_minmax(0,2fr)_4.5rem_5rem]",
        )}
      >
        {/* Expand chevron or icon */}
        {hasDetail ? (
          <CollapsibleTrigger className="flex items-center justify-center">
            <ChevronRight
              className={cn(
                "size-3.5 text-muted-foreground transition-transform",
                expanded && "rotate-90",
              )}
            />
          </CollapsibleTrigger>
        ) : (
          <Cog className="size-3.5 text-muted-foreground" />
        )}

        {/* Name + status dot */}
        <div className="flex min-w-0 items-center gap-2">
          <StatusDot state={isRunning ? "running" : "idle"} animate={isRunning} />
          <span className="truncate font-medium">{displayName(handler.name)}</span>
        </div>

        {/* Status / description */}
        <div className="hidden min-w-0 overflow-hidden sm:block">
          {isRunning && handler.activeRun ? (
            <p className="truncate text-xs text-muted-foreground">
              {handler.activeRun.method}
              {handler.activeRun.key && ` · ${handler.activeRun.key}`}
              {handler.activeRun.startedAt &&
                ` · Started ${formatRelativeTime(handler.activeRun.startedAt)}`}
            </p>
          ) : (
            <p className="truncate text-xs text-muted-foreground">{handler.description}</p>
          )}
        </div>

        {/* Status label */}
        <span
          className={cn(
            "hidden text-right text-xs sm:block",
            isRunning ? "font-medium text-foreground" : "text-muted-foreground",
          )}
        >
          {isRunning ? "Running" : "Idle"}
        </span>

        {/* Actions */}
        <div className="flex shrink-0 items-center justify-end">
          {isRunning && handler.activeRun ? (
            <CancelButton
              onClick={() => onCancelRun(handler.activeRun!.runId)}
              isPending={cancelPending}
            />
          ) : (
            <TriggerHandlerPopover handler={handler} />
          )}
        </div>
      </div>

      {/* Expanded detail row */}
      <CollapsibleContent>
        {handler.activeRun && (
          <div className="border-b bg-muted/40 px-4 py-2.5">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
              <Stat label="" value={handler.activeRun.method} />
              {handler.activeRun.key && (
                <>
                  {DOT_SEP}
                  <Stat label="" value={handler.activeRun.key} />
                </>
              )}
              {handler.activeRun.startedAt && (
                <>
                  {DOT_SEP}
                  <Stat
                    label=""
                    value={`Started ${formatRelativeTime(handler.activeRun.startedAt)}`}
                  />
                </>
              )}
            </div>
          </div>
        )}
      </CollapsibleContent>
    </Collapsible>
  );
};
