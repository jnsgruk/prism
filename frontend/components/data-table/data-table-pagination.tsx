import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { ChevronLeft, ChevronRight } from "lucide-react";

const PAGE_SIZE_OPTIONS = ["25", "50", "100"];

interface DataTablePaginationProps {
  totalCount: number;
  pageSize: number;
  pageIndex: number;
  hasNextPage: boolean;
  onPageSizeChange: (size: number) => void;
  onPreviousPage: () => void;
  onNextPage: () => void;
}

export const DataTablePagination = ({
  totalCount,
  pageSize,
  pageIndex,
  hasNextPage,
  onPageSizeChange,
  onPreviousPage,
  onNextPage,
}: DataTablePaginationProps): React.ReactElement => {
  const start = pageIndex * pageSize + 1;
  const end = Math.min(start + pageSize - 1, totalCount);

  return (
    <div className="flex items-center justify-between">
      <p className="text-sm text-muted-foreground">
        {totalCount > 0 ? `${start}\u2013${end} of ${totalCount}` : "No results"}
      </p>
      <div className="flex items-center gap-3">
        <div className="flex items-center gap-1.5">
          <span className="text-sm text-muted-foreground">Rows</span>
          <Select
            value={String(pageSize)}
            onValueChange={(v) => v !== null && onPageSizeChange(Number(v))}
          >
            <SelectTrigger>
              <SelectValue>{pageSize}</SelectValue>
            </SelectTrigger>
            <SelectContent>
              {PAGE_SIZE_OPTIONS.map((size) => (
                <SelectItem key={size} value={size}>
                  {size}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
        <div className="flex items-center gap-1">
          <Button
            variant="outline"
            size="icon-sm"
            onClick={onPreviousPage}
            disabled={pageIndex === 0}
          >
            <ChevronLeft className="size-4" />
          </Button>
          <Button variant="outline" size="icon-sm" onClick={onNextPage} disabled={!hasNextPage}>
            <ChevronRight className="size-4" />
          </Button>
        </div>
      </div>
    </div>
  );
};
