import { Button } from "@/components/ui/button";
import { Loader2, Play, Square } from "lucide-react";

export const CancelButton = ({
  onClick,
  isPending,
}: {
  onClick: () => void;
  isPending: boolean;
}): React.ReactElement => (
  <Button
    variant="ghost"
    size="sm"
    className="h-7 text-destructive hover:text-destructive"
    onClick={onClick}
    disabled={isPending}
  >
    {isPending ? <Loader2 className="size-3.5 animate-spin" /> : <Square className="size-3.5" />}
    <span className="ml-1 hidden sm:inline">Cancel</span>
  </Button>
);

export const RunButton = ({
  onClick,
  isPending,
}: {
  onClick: () => void;
  isPending: boolean;
}): React.ReactElement => (
  <Button variant="ghost" size="sm" className="h-7" onClick={onClick} disabled={isPending}>
    {isPending ? <Loader2 className="size-3.5 animate-spin" /> : <Play className="size-3.5" />}
    <span className="ml-1 hidden sm:inline">Run</span>
  </Button>
);
