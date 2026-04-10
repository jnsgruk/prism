import type { ColumnDef } from "@tanstack/react-table";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { DataTable } from "./data-table";

interface TestRow {
  id: string;
  name: string;
  value: number;
}

const columns: ColumnDef<TestRow>[] = [
  { accessorKey: "name", header: "Name" },
  { accessorKey: "value", header: "Value" },
];

const sortableColumns: ColumnDef<TestRow>[] = [
  { accessorKey: "name", header: "Name", enableSorting: true },
  { accessorKey: "value", header: "Value", enableSorting: true },
];

const data: TestRow[] = [
  { id: "1", name: "Alice", value: 10 },
  { id: "2", name: "Bob", value: 20 },
  { id: "3", name: "Charlie", value: 30 },
];

describe("DataTable", () => {
  it("renders column headers", () => {
    render(<DataTable columns={columns} data={data} />);
    expect(screen.getByText("Name")).toBeInTheDocument();
    expect(screen.getByText("Value")).toBeInTheDocument();
  });

  it("renders row data", () => {
    render(<DataTable columns={columns} data={data} />);
    expect(screen.getAllByText("Alice").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("Bob").length).toBeGreaterThanOrEqual(1);
    expect(screen.getAllByText("Charlie").length).toBeGreaterThanOrEqual(1);
  });

  it("shows empty state when data is empty", () => {
    render(<DataTable columns={columns} data={[]} />);
    expect(screen.getByText("No results.")).toBeInTheDocument();
  });

  it("fires row click callback", () => {
    const onClick = vi.fn<(row: TestRow) => void>();
    const { container } = render(<DataTable columns={columns} data={data} onRowClick={onClick} />);

    // Click on the table row containing Alice
    const rows = container.querySelectorAll("tr[class*='cursor-pointer']");
    fireEvent.click(rows[0]!);
    expect(onClick).toHaveBeenCalledWith(data[0]);
  });

  it("calls onSortingChange when sortable header is clicked", () => {
    const onSortingChange = vi.fn<() => void>();
    const { container } = render(
      <DataTable columns={sortableColumns} data={data} sorting={[]} onSortingChange={onSortingChange} />,
    );

    // Click the first sortable header button
    const buttons = container.querySelectorAll("button");
    fireEvent.click(buttons[0]!);
    expect(onSortingChange).toHaveBeenCalled();
  });

  it("shows sort direction indicator when sorted", () => {
    render(
      <DataTable
        columns={sortableColumns}
        data={data}
        sorting={[{ id: "name", desc: false }]}
        onSortingChange={vi.fn<() => void>()}
      />,
    );

    // Should show ascending arrow
    expect(screen.getByText("↑")).toBeInTheDocument();
  });
});
