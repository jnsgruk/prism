import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { CircleOff, CirclePlay, MoreHorizontal } from "lucide-react";

export const SourceOverflowMenu = ({
  sourceId,
  enabled,
  onToggleEnabled,
}: {
  sourceId?: string;
  enabled: boolean;
  onToggleEnabled?: (sourceId: string, enabled: boolean) => void;
}): React.ReactElement | null => {
  if (!sourceId || !onToggleEnabled) return null;

  return (
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
      </DropdownMenuContent>
    </DropdownMenu>
  );
};
