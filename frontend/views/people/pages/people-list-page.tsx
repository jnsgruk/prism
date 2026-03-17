import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Alert } from "@/components/ui/alert";
import type { ColumnDef, SortingState } from "@tanstack/react-table";
import { Search } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";

import type { Person } from "@ps/api/gen/prism/v1/org_pb";

import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { PageHeader } from "@/components/page-header";
import { usePaginatedPeople } from "@/views/teams/hooks/use-teams";

type Filter = "all" | "unassigned" | "inactive";

const columns: ColumnDef<Person, unknown>[] = [
  {
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
  },
  {
    accessorKey: "email",
    header: "Email",
    enableSorting: false,
    cell: ({ row }) => (
      <span className="text-muted-foreground">{row.original.email || "\u2014"}</span>
    ),
  },
  {
    accessorKey: "level",
    header: "Level",
    enableSorting: false,
    cell: ({ row }) => (
      <span className="text-muted-foreground">{row.original.level || "\u2014"}</span>
    ),
  },
  {
    accessorKey: "team_name",
    header: "Team",
    enableSorting: true,
    cell: ({ row }) =>
      row.original.teamName ? (
        <Badge variant="secondary">{row.original.teamName}</Badge>
      ) : (
        <span className="text-muted-foreground">{"\u2014"}</span>
      ),
  },
  {
    id: "identities",
    header: "Platforms",
    enableSorting: false,
    cell: ({ row }) => (
      <div className="flex flex-wrap gap-1">
        {row.original.identities.map((id) => (
          <Badge key={`${id.platform}-${id.username}`} variant="outline" className="text-[10px]">
            {id.platform}
          </Badge>
        ))}
      </div>
    ),
  },
];

const PeopleListPage = (): React.ReactElement => {
  const navigate = useNavigate();
  const [filter, setFilter] = useState<Filter>("all");
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [pageSize, setPageSize] = useState(25);
  const [pageIndex, setPageIndex] = useState(0);
  const [pageTokens, setPageTokens] = useState<string[]>([""]);
  const [sorting, setSorting] = useState<SortingState>([{ id: "name", desc: false }]);

  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 300);
    return (): void => {
      clearTimeout(timer);
    };
  }, [search]);

  useEffect(() => {
    setPageIndex(0);
    setPageTokens([""]);
  }, [debouncedSearch, filter, pageSize, sorting]);

  const sortField = sorting[0]?.id;
  const sortDesc = sorting[0]?.desc ?? false;

  const { data, isLoading, isError, error } = usePaginatedPeople({
    search: debouncedSearch || undefined,
    filter: filter === "all" ? undefined : filter,
    pageSize,
    pageToken: pageTokens[pageIndex] ?? "",
    sortField,
    sortDesc,
  });

  const people = data?.people ?? [];
  const totalCount = data?.pagination?.totalCount ?? 0;
  const nextPageToken = data?.pagination?.nextPageToken ?? "";

  const handleNextPage = useCallback(() => {
    if (!nextPageToken) return;
    setPageTokens((prev) => {
      const next = [...prev];
      next[pageIndex + 1] = nextPageToken;
      return next;
    });
    setPageIndex((i) => i + 1);
  }, [nextPageToken, pageIndex]);

  const handlePrevPage = useCallback(() => {
    setPageIndex((i) => Math.max(0, i - 1));
  }, []);

  const handlePageSizeChange = useCallback((size: number) => {
    setPageSize(size);
  }, []);

  return (
    <>
      <PageHeader title="People" description="Browse and search people across the organisation" />
      <div className="min-w-0 flex-1 space-y-4 overflow-y-auto p-6">
        {/* Filters */}
        <div className="flex flex-wrap items-center gap-3">
          <div className="relative">
            <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
            <Input
              placeholder="Search people..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="h-8 w-64 pl-8 text-sm"
            />
          </div>
          <div className="flex items-center gap-1">
            <Button
              variant={filter === "all" ? "default" : "outline"}
              size="sm"
              onClick={() => setFilter("all")}
            >
              All
            </Button>
            <Button
              variant={filter === "unassigned" ? "default" : "outline"}
              size="sm"
              onClick={() => setFilter("unassigned")}
            >
              Unassigned
            </Button>
            <Button
              variant={filter === "inactive" ? "default" : "outline"}
              size="sm"
              onClick={() => setFilter("inactive")}
            >
              Inactive
            </Button>
          </div>
        </div>

        {/* Loading */}
        {isLoading && people.length === 0 && (
          <div className="space-y-3">
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
            <Skeleton className="h-10 w-full" />
          </div>
        )}

        {/* Error */}
        {isError && (
          <Alert variant="destructive">{error?.message ?? "Failed to load people"}</Alert>
        )}

        {/* Table */}
        {!isLoading && !isError && (
          <>
            <div className="overflow-x-auto rounded-md border">
              <DataTable
                columns={columns}
                data={people}
                sorting={sorting}
                onSortingChange={setSorting}
                onRowClick={(person) => navigate(`/people/${person.id}`)}
              />
            </div>
            <DataTablePagination
              totalCount={totalCount}
              pageSize={pageSize}
              pageIndex={pageIndex}
              hasNextPage={!!nextPageToken}
              onPageSizeChange={handlePageSizeChange}
              onPreviousPage={handlePrevPage}
              onNextPage={handleNextPage}
            />
          </>
        )}
      </div>
    </>
  );
};

export default PeopleListPage;
