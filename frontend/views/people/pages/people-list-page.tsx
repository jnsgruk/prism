import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { PageHeader } from "@/components/page-header";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandGroup, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Skeleton } from "@/components/ui/skeleton";
import { useDebouncedValue } from "@/lib/hooks/use-debounced-value";
import { flattenTree, useGetTeamTree, usePaginatedPeople } from "@/lib/hooks/use-org";
import { personNameColumn, personTeamColumn, personIdentitiesColumn } from "@/views/people/components/person-columns";
import type { SortingState } from "@tanstack/react-table";
import { ChevronsUpDown, Search, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

const columns = [personNameColumn, personTeamColumn, personIdentitiesColumn];

const PeopleListPage = (): React.ReactElement => {
  const navigate = useNavigate();
  const [teamId, setTeamId] = useState<string | undefined>(undefined);
  const [teamOpen, setTeamOpen] = useState(false);
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebouncedValue(search);
  const [pageSize, setPageSize] = useState(25);
  const [pageIndex, setPageIndex] = useState(0);
  const [pageTokens, setPageTokens] = useState([""]);
  const [sorting, setSorting] = useState<SortingState>([{ id: "name", desc: false }]);

  const { data: treeData } = useGetTeamTree();
  const flatTeams = useMemo(() => flattenTree(treeData?.roots ?? []), [treeData?.roots]);

  const selectedTeamName = useMemo(() => flatTeams.find((ft) => ft.team.id === teamId)?.team.name, [flatTeams, teamId]);

  useEffect(() => {
    setPageIndex(0);
    setPageTokens([""]);
  }, [debouncedSearch, teamId, pageSize, sorting]);

  const sortField = sorting[0]?.id;
  const sortDesc = sorting[0]?.desc ?? false;

  const { data, isLoading, isError, error } = usePaginatedPeople({
    search: debouncedSearch || undefined,
    teamId,
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
          <Popover open={teamOpen} onOpenChange={setTeamOpen}>
            <PopoverTrigger render={<Button variant="outline" size="sm" className="h-8 w-48 justify-between" />}>
              <span className="truncate">{selectedTeamName ?? "All teams"}</span>
              {teamId ? (
                <X
                  className="size-3.5 shrink-0 text-muted-foreground hover:text-foreground"
                  onClick={(e) => {
                    e.stopPropagation();
                    setTeamId(undefined);
                  }}
                />
              ) : (
                <ChevronsUpDown className="size-3.5 shrink-0 text-muted-foreground" />
              )}
            </PopoverTrigger>
            <PopoverContent className="w-56 p-0" align="start">
              <Command>
                <CommandInput placeholder="Search teams..." />
                <CommandList>
                  <CommandEmpty>No teams found.</CommandEmpty>
                  <CommandGroup>
                    {flatTeams.map((ft) => (
                      <CommandItem
                        key={ft.team.id}
                        value={ft.team.name}
                        data-checked={teamId === ft.team.id}
                        onSelect={() => {
                          setTeamId(teamId === ft.team.id ? undefined : ft.team.id);
                          setTeamOpen(false);
                        }}
                      >
                        <span style={{ paddingLeft: `${ft.depth * 12}px` }}>{ft.team.name}</span>
                      </CommandItem>
                    ))}
                  </CommandGroup>
                </CommandList>
              </Command>
            </PopoverContent>
          </Popover>
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
        {isError && <Alert variant="destructive">{error?.message ?? "Failed to load people"}</Alert>}

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
