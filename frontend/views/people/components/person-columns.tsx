import { Badge } from "@/components/ui/badge";
import type { ColumnDef } from "@tanstack/react-table";

import type { Person } from "@ps/api/gen/canonical/prism/v1/org_pb";

export const personNameColumn: ColumnDef<Person, unknown> = {
  accessorKey: "name",
  header: "Name",
  enableSorting: true,
  cell: ({ row }) => (
    <div className="flex items-center gap-2">
      <span className="font-medium">{row.original.name}</span>
      {!row.original.active && (
        <Badge variant="destructive" className="text-[10px]">
          Inactive
        </Badge>
      )}
    </div>
  ),
};

export const personTeamColumn: ColumnDef<Person, unknown> = {
  accessorKey: "team_name",
  header: "Team",
  enableSorting: true,
  cell: ({ row }) =>
    row.original.teamName ? (
      <Badge variant="secondary">{row.original.teamName}</Badge>
    ) : (
      <span className="text-muted-foreground">{"\u2014"}</span>
    ),
};

export const personIdentitiesColumn: ColumnDef<Person, unknown> = {
  id: "identities",
  header: "Identities",
  enableSorting: false,
  cell: ({ row }) => {
    const count = row.original.identities.length;
    return count > 0 ? (
      <Badge variant="outline" className="text-[10px]">
        {count} {count === 1 ? "identity" : "identities"}
      </Badge>
    ) : (
      <span className="text-muted-foreground">{"\u2014"}</span>
    );
  },
};
