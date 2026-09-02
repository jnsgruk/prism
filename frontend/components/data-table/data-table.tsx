import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { flexRender, type OnChangeFn, type RowData, type SortingState } from "@tanstack/react-table";
import {
  type LegacyColumnDef as ColumnDef,
  getCoreRowModel as createCoreRowModel,
  useLegacyTable as useReactTable,
} from "@tanstack/react-table/legacy";
import { ArrowUpDown } from "lucide-react";

interface DataTableProps<TData extends RowData> {
  columns: ColumnDef<TData>[];
  data: TData[];
  sorting?: SortingState;
  onSortingChange?: OnChangeFn<SortingState>;
  onRowClick?: (row: TData) => void;
}

export const DataTable = <TData extends RowData>({
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
    getCoreRowModel: createCoreRowModel<TData>(),
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
