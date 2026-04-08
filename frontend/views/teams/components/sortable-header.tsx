import { TableHead } from "@/components/ui/table";
import { ArrowUpDown } from "lucide-react";

export type SortField =
  | "name"
  | "throughput"
  | "reviewP75"
  | "cycleTime"
  | "discourseTopics"
  | "discoursePosts";
export type SortDir = "asc" | "desc";

export const SortableHeader = ({
  field,
  current,
  dir,
  onSort,
  children,
}: {
  field: SortField;
  current: SortField;
  dir: SortDir;
  onSort: (field: SortField) => void;
  children: React.ReactNode;
}): React.ReactElement => (
  <TableHead>
    <button className="flex items-center gap-1 text-left font-medium" onClick={() => onSort(field)}>
      {children}
      <ArrowUpDown
        className={`size-3 ${current === field ? "text-foreground" : "text-muted-foreground/50"}`}
      />
      {current === field && <span className="text-xs">{dir === "asc" ? "\u2191" : "\u2193"}</span>}
    </button>
  </TableHead>
);
