import { Button } from "@/components/ui/button";
import type { PdfNavigation } from "@/views/ask/hooks/workspace-preview.types";
import { ChevronLeft, ChevronRight } from "lucide-react";

export const WorkspacePdfNavigation = ({ page, pageCount, onPageChange }: PdfNavigation): React.ReactElement => (
  <div className="flex min-w-0 items-center justify-center gap-1">
    <Button
      variant="ghost"
      size="icon"
      className="size-6 shrink-0"
      aria-label="Previous PDF page"
      disabled={page <= 1 || !pageCount}
      onClick={() => onPageChange(page - 1)}
    >
      <ChevronLeft className="size-3.5" />
    </Button>
    <span className="text-xs tabular-nums" aria-live="polite">
      Page {page} of {pageCount ?? "…"}
    </span>
    <Button
      variant="ghost"
      size="icon"
      className="size-6 shrink-0"
      aria-label="Next PDF page"
      disabled={!pageCount || page >= pageCount}
      onClick={() => onPageChange(page + 1)}
    >
      <ChevronRight className="size-3.5" />
    </Button>
  </div>
);
