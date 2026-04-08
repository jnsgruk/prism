import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Skeleton } from "@/components/ui/skeleton";
import { useDebouncedValue } from "@/lib/hooks/use-debounced-value";
import { flattenTree, useGetTeamTree, usePaginatedPeople } from "@/lib/hooks/use-org";
import { UNASSIGNED_TEAM_ID } from "@/views/admin/components/org-team-sidebar";
import { useUnassignGithubTeam } from "@/views/admin/hooks/use-admin";
import { flattenTeams } from "@/views/admin/lib/team-utils";
import { personNameColumn, personTeamColumn } from "@/views/people/components/person-columns";
import { GithubTeamPickerDialog } from "@/views/teams/components/github-team-picker-dialog";
import { TeamMappingSuggestions } from "@/views/teams/components/team-mapping-suggestions";
import { useListTeamGithubTeams } from "@/views/teams/hooks/use-teams";
import type { SortingState } from "@tanstack/react-table";
import { ChevronsUpDown, Ellipsis, GitBranch, Pencil, Plus, Trash2, Users, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import { PersonFilter } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { Person, Team } from "@ps/api/gen/canonical/prism/v1/org_pb";

type Filter = "all" | "unassigned" | "inactive";

const filterToEnum = (f: Filter): PersonFilter | undefined => {
  switch (f) {
    case "unassigned":
      return PersonFilter.UNASSIGNED;
    case "inactive":
      return PersonFilter.INACTIVE;
    default:
      return undefined;
  }
};

const allColumns = [personNameColumn, personTeamColumn];
const teamColumns = [personNameColumn];

const TeamHeader = ({
  team,
  onSelectTeam,
  onEditTeam,
  onDeleteTeam,
}: {
  team: Team;
  onSelectTeam: (id: string | null) => void;
  onEditTeam?: (team: Team) => void;
  onDeleteTeam?: (team: Team) => void;
}): React.ReactElement => {
  const memberCount = team.totalMemberCount > 0 ? team.totalMemberCount : team.memberCount;

  return (
    <div className="rounded-lg border p-4">
      <div className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <h3 className="truncate text-lg font-semibold">{team.name}</h3>
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-muted-foreground">
            <span className="flex items-center gap-1.5">
              <Users className="size-3.5" />
              {memberCount} {memberCount === 1 ? "member" : "members"}
            </span>
            {team.leadName && <span>Lead: {team.leadName}</span>}
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {(onEditTeam || onDeleteTeam) && (
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" />}>
                <Ellipsis className="size-4" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {onEditTeam && (
                  <DropdownMenuItem onClick={() => onEditTeam(team)}>
                    <Pencil className="size-3.5" />
                    Edit
                  </DropdownMenuItem>
                )}
                {onDeleteTeam && (
                  <DropdownMenuItem className="text-destructive" onClick={() => onDeleteTeam(team)}>
                    <Trash2 className="size-3.5" />
                    Delete
                  </DropdownMenuItem>
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          )}
          <Button
            variant="ghost"
            size="icon-sm"
            title="Clear selection"
            onClick={() => onSelectTeam(null)}
          >
            <X className="size-3.5" />
          </Button>
        </div>
      </div>
    </div>
  );
};

const LinkedTeamsCard = ({ teamId }: { teamId: string }): React.ReactElement => {
  const { data: githubTeams } = useListTeamGithubTeams(teamId);
  const unassign = useUnassignGithubTeam();
  const [pickerOpen, setPickerOpen] = useState(false);

  const ghCount = githubTeams?.length ?? 0;

  return (
    <div className="space-y-3 rounded-lg border p-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">Linked Teams</h3>
        <Button variant="outline" size="sm" onClick={() => setPickerOpen(true)}>
          <Plus className="size-3.5" />
          Link
        </Button>
      </div>
      {ghCount === 0 ? (
        <p className="text-sm text-muted-foreground">
          No GitHub teams linked. Link a team to scope ingestion.
        </p>
      ) : (
        <div className="space-y-2">
          {githubTeams?.map((gt) => (
            <div
              key={gt.id}
              className="flex items-center justify-between gap-2 rounded border px-3 py-2"
            >
              <div className="flex min-w-0 items-center gap-2">
                <GitBranch className="size-3.5 shrink-0 text-muted-foreground" />
                <span className="truncate text-sm">
                  {gt.githubOrg}/{gt.slug}
                </span>
              </div>
              <div className="flex items-center gap-1">
                <span className="text-xs text-muted-foreground">
                  {Number(gt.memberCount)} members · {Number(gt.repoCount)} repos
                </span>
                <Button
                  variant="ghost"
                  size="icon-sm"
                  onClick={() => unassign.mutate({ teamId, githubTeamId: gt.id })}
                >
                  <X className="size-3" />
                </Button>
              </div>
            </div>
          ))}
        </div>
      )}
      <TeamMappingSuggestions teamId={teamId} />
      <GithubTeamPickerDialog
        teamId={teamId}
        open={pickerOpen}
        onOpenChange={setPickerOpen}
        alreadyAssigned={githubTeams?.map((t) => t.id) ?? []}
      />
    </div>
  );
};

export const OrgPeoplePanel = ({
  teamId,
  onSelectTeam,
  onSelectPerson,
  onEditTeam,
  onDeleteTeam,
}: {
  teamId: string | null;
  onSelectTeam: (id: string | null) => void;
  onSelectPerson: (person: Person) => void;
  onEditTeam?: (team: Team) => void;
  onDeleteTeam?: (team: Team) => void;
}): React.ReactElement => {
  const { data: tree } = useGetTeamTree();
  const [filter, setFilter] = useState<Filter>("all");
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebouncedValue(search);
  const [pageSize, setPageSize] = useState(25);
  const [pageIndex, setPageIndex] = useState(0);
  const [pageTokens, setPageTokens] = useState<string[]>([""]);
  const [sorting, setSorting] = useState<SortingState>([{ id: "name", desc: false }]);
  const [teamPickerOpen, setTeamPickerOpen] = useState(false);

  const allTeams = useMemo(() => (tree ? flattenTeams(tree.roots) : []), [tree]);
  const flatTeams = useMemo(() => flattenTree(tree?.roots ?? []), [tree?.roots]);

  const isUnassigned = teamId === UNASSIGNED_TEAM_ID;
  const isTeamSelected = !!teamId && !isUnassigned;

  // Derive the effective filter and teamId for the query.
  const effectiveFilter = isUnassigned ? PersonFilter.UNASSIGNED : filterToEnum(filter);
  const effectiveTeamId = isTeamSelected ? teamId : undefined;

  // Find the selected team for the inline header.
  const selectedTeam = useMemo(() => {
    if (!isTeamSelected) return null;
    return allTeams.find((t) => t.id === teamId) ?? null;
  }, [teamId, isTeamSelected, allTeams]);

  // Hide the Team column when viewing a specific team's members.
  const columns = isTeamSelected ? teamColumns : allColumns;

  // Reset to first page when filters change.
  useEffect(() => {
    setPageIndex(0);
    setPageTokens([""]);
  }, [debouncedSearch, filter, pageSize, sorting, teamId]);

  // When sidebar selects "Unassigned", sync the filter buttons.
  useEffect(() => {
    if (isUnassigned) setFilter("all");
  }, [isUnassigned]);

  const sortField = sorting[0]?.id;
  const sortDesc = sorting[0]?.desc ?? false;

  const { data, isLoading, isError, error } = usePaginatedPeople({
    search: debouncedSearch || undefined,
    filter: effectiveFilter,
    teamId: effectiveTeamId,
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

  const handleFilterChange = (f: Filter): void => {
    setFilter(f);
    // If user clicks "Unassigned" filter button while a team is selected, clear team selection.
    if (f === "unassigned" && teamId && !isUnassigned) {
      onSelectTeam(UNASSIGNED_TEAM_ID);
    }
    // If switching away from unassigned filter while unassigned pseudo-node is selected, go to all.
    if (f !== "unassigned" && isUnassigned) {
      onSelectTeam(null);
    }
  };

  if (isLoading && people.length === 0) {
    return (
      <div className="space-y-3">
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
      </div>
    );
  }

  if (isError) {
    return <Alert variant="destructive">{error?.message ?? "Failed to load people"}</Alert>;
  }

  return (
    <div className="space-y-4">
      {/* Mobile-only team picker (hidden on md+) */}
      <div className="md:hidden">
        <Popover open={teamPickerOpen} onOpenChange={setTeamPickerOpen}>
          <PopoverTrigger
            render={<Button variant="outline" size="sm" className="h-8 w-full justify-between" />}
          >
            <span className="truncate">
              {isUnassigned ? "Unassigned" : (selectedTeam?.name ?? "All teams")}
            </span>
            {teamId ? (
              <X
                className="size-3.5 shrink-0 text-muted-foreground hover:text-foreground"
                onClick={(e) => {
                  e.stopPropagation();
                  onSelectTeam(null);
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
                  <CommandItem
                    value="All teams"
                    data-checked={teamId === null}
                    onSelect={() => {
                      onSelectTeam(null);
                      setTeamPickerOpen(false);
                    }}
                  >
                    All teams
                  </CommandItem>
                  {flatTeams.map((ft) => (
                    <CommandItem
                      key={ft.team.id}
                      value={ft.team.name}
                      data-checked={teamId === ft.team.id}
                      onSelect={() => {
                        onSelectTeam(teamId === ft.team.id ? null : ft.team.id);
                        setTeamPickerOpen(false);
                      }}
                    >
                      <span style={{ paddingLeft: `${ft.depth * 12}px` }}>{ft.team.name}</span>
                    </CommandItem>
                  ))}
                  <CommandItem
                    value="Unassigned"
                    data-checked={isUnassigned}
                    onSelect={() => {
                      onSelectTeam(UNASSIGNED_TEAM_ID);
                      setTeamPickerOpen(false);
                    }}
                  >
                    Unassigned
                  </CommandItem>
                </CommandGroup>
              </CommandList>
            </Command>
          </PopoverContent>
        </Popover>
      </div>

      {/* Inline team header */}
      {selectedTeam && (
        <TeamHeader
          team={selectedTeam}
          onSelectTeam={onSelectTeam}
          onEditTeam={onEditTeam}
          onDeleteTeam={onDeleteTeam}
        />
      )}

      {/* Linked GitHub teams */}
      {isTeamSelected && <LinkedTeamsCard teamId={teamId} />}

      {/* Empty team placeholder */}
      {isTeamSelected && totalCount === 0 && !debouncedSearch ? (
        <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
          <Users className="mb-3 size-10 text-muted-foreground" />
          <p className="mb-1 font-medium">No team members</p>
          <p className="text-sm text-muted-foreground">
            People will appear here once they are assigned to this team.
          </p>
        </div>
      ) : (
        <div className="space-y-4 rounded-lg border p-4">
          <h3 className="text-sm font-medium">People</h3>
          {/* Search + filter bar */}
          <div className="flex flex-wrap items-center gap-3">
            <Input
              placeholder="Filter people..."
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              className="min-w-0 flex-1"
            />
            {!isTeamSelected && (
              <div className="flex items-center gap-1">
                <Button
                  variant={filter === "all" && !isUnassigned ? "default" : "outline"}
                  size="sm"
                  onClick={() => handleFilterChange("all")}
                >
                  All
                </Button>
                <Button
                  variant={filter === "unassigned" || isUnassigned ? "default" : "outline"}
                  size="sm"
                  onClick={() => handleFilterChange("unassigned")}
                >
                  Unassigned
                </Button>
                <Button
                  variant={filter === "inactive" ? "default" : "outline"}
                  size="sm"
                  onClick={() => handleFilterChange("inactive")}
                >
                  Inactive
                </Button>
              </div>
            )}
          </div>

          <DataTable
            columns={columns}
            data={people}
            sorting={sorting}
            onSortingChange={setSorting}
            onRowClick={onSelectPerson}
          />

          <DataTablePagination
            totalCount={totalCount}
            pageSize={pageSize}
            pageIndex={pageIndex}
            hasNextPage={!!nextPageToken}
            onPageSizeChange={handlePageSizeChange}
            onPreviousPage={handlePrevPage}
            onNextPage={handleNextPage}
          />
        </div>
      )}
    </div>
  );
};
