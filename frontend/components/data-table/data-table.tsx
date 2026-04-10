import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import {
  type ColumnDef,
  type OnChangeFn,
  type SortingState,
  flexRender,
  getCoreRowModel as createCoreRowModel,
  useReactTable,
} from "@tanstack/react-table";
import { ArrowUpDown } from "lucide-react";

const coreRowModel = createCoreRowModel();

interface DataTableProps<TData> {
  columns: ColumnDef<TData, unknown>[];
  data: TData[];
  sorting?: SortingState;
  onSortingChange?: OnChangeFn<SortingState>;
  onRowClick?: (row: TData) => void;
}

export const DataTable = <TData,>({
  columns,
  data,
  sorting,
  onSortingChange,
  onRowClick,
}: DataTableProps<TData>): React.ReactElement => {
  const table = useReactTable({
    data,
    columns,
    state: { sorting: sorting ?? [] },
    onSortingChange,
    getCoreRowModel: coreRowModel,
    manualSorting: true,
  });

  return (
    <Table>
      <TableHeader>
        {table.getHeaderGroups().map((hg) => (
          <TableRow key={hg.id}>
            {hg.headers.map((header) => (
              <TableHead key={header.id}>
                {((): React.ReactNode => {
                  if (header.isPlaceholder) return null;
                  const rendered = flexRender(header.column.columnDef.header, header.getContext());
                  if (!header.column.getCanSort()) return rendered;
                  return (
                    <button
                      className="flex items-center gap-1 text-left font-medium"
                      onClick={header.column.getToggleSortingHandler()}
                    >
                      {rendered}
                      <ArrowUpDown
                        className={`size-3 ${
                          header.column.getIsSorted() ? "text-foreground" : "text-muted-foreground/50"
                        }`}
                      />
                      {header.column.getIsSorted() === "asc" && <span className="text-xs">&uarr;</span>}
                      {header.column.getIsSorted() === "desc" && <span className="text-xs">&darr;</span>}
                    </button>
                  );
                })()}
              </TableHead>
            ))}
          </TableRow>
        ))}
      </TableHeader>
      <TableBody>
        {table.getRowModel().rows.length === 0 ? (
          <TableRow>
            <TableCell colSpan={columns.length} className="text-center text-muted-foreground">
              No results.
            </TableCell>
          </TableRow>
        ) : (
          table.getRowModel().rows.map((row) => (
            <TableRow
              key={row.id}
              className={onRowClick ? "cursor-pointer" : undefined}
              onClick={() => onRowClick?.(row.original)}
            >
              {row.getVisibleCells().map((cell) => (
                <TableCell key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</TableCell>
              ))}
            </TableRow>
          ))
        )}
      </TableBody>
    </Table>
  );
};
