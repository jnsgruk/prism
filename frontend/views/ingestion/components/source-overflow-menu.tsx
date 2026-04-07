import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { CircleOff, CirclePlay, MoreHorizontal, Play, RotateCcw, Square } from "lucide-react";
import { useState } from "react";

import { BackfillDialog } from "./backfill-dialog";

export const SourceOverflowMenu = ({
  sourceName,
  sourceId,
  isActive,
  enabled,
  onTriggerRun,
  onCancelRun,
  onToggleEnabled,
  onAction,
}: {
  sourceName: string;
  sourceId?: string;
  isActive: boolean;
  enabled: boolean;
  onTriggerRun?: (name: string) => void;
  onCancelRun?: (name: string) => void;
  onToggleEnabled?: (sourceId: string, enabled: boolean) => void;
  onAction?: () => void;
}): React.ReactElement => {
  const [showBackfill, setShowBackfill] = useState(false);

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger
          render={
            <Button
              variant="ghost"
              size="sm"
              className="h-7 w-7 p-0 opacity-0 group-hover:opacity-100 data-popup-open:opacity-100 sm:opacity-0"
            />
          }
        >
          <MoreHorizontal className="size-4" />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end" side="bottom">
          {isActive && (
            <DropdownMenuItem onClick={() => onCancelRun?.(sourceName)}>
              <Square className="size-3.5" />
              Cancel
            </DropdownMenuItem>
          )}
          {!isActive && enabled && (
            <>
              <DropdownMenuItem onClick={() => onTriggerRun?.(sourceName)}>
                <Play className="size-3.5" />
                Run
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => setShowBackfill(true)}>
                <RotateCcw className="size-3.5" />
                Backfill…
              </DropdownMenuItem>
            </>
          )}
          {sourceId && onToggleEnabled && (
            <>
              <DropdownMenuSeparator />
              <DropdownMenuItem onClick={() => onToggleEnabled(sourceId, !enabled)}>
                {enabled ? (
                  <>
                    <CircleOff className="size-3.5" />
                    Disable
                  </>
                ) : (
                  <>
                    <CirclePlay className="size-3.5" />
                    Enable
                  </>
                )}
              </DropdownMenuItem>
            </>
          )}
        </DropdownMenuContent>
      </DropdownMenu>

      {enabled && (
        <BackfillDialog
          sourceName={sourceName}
          open={showBackfill}
          onOpenChange={setShowBackfill}
          onAction={onAction}
        />
      )}
    </>
  );
};
