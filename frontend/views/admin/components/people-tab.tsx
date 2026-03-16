import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import type { ColumnDef, SortingState } from "@tanstack/react-table";
import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";

import type { Person, Team } from "@ps/api/gen/prism/v1/org_pb";

import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import {
  useUpdatePerson,
  useDeactivatePerson,
  useReactivatePerson,
  useAssignPersonToTeam,
  useRemovePersonFromTeam,
} from "@/views/admin/hooks/use-admin";
import { flattenTeams } from "@/views/admin/lib/team-utils";
import { useGetTeamTree, usePaginatedPeople } from "@/views/teams/hooks/use-teams";

type Filter = "all" | "unassigned" | "inactive";

const columns: ColumnDef<Person, unknown>[] = [
  {
    accessorKey: "name",
    header: "Name",
    enableSorting: true,
    cell: ({ row }) => (
      <div className="flex items-center gap-2">
        <span className="font-medium">{row.original.name}</span>
        {!row.original.active && <Badge variant="destructive">Inactive</Badge>}
      </div>
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
        <span className="text-muted-foreground">&mdash;</span>
      ),
  },
];

export const PeopleTab = (): React.ReactElement => {
  const { data: tree } = useGetTeamTree();
  const [filter, setFilter] = useState<Filter>("all");
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [pageSize, setPageSize] = useState(25);
  const [pageIndex, setPageIndex] = useState(0);
  const [pageTokens, setPageTokens] = useState<string[]>([""]);
  const [sorting, setSorting] = useState<SortingState>([{ id: "name", desc: false }]);
  const [selectedPerson, setSelectedPerson] = useState<Person | null>(null);

  const allTeams = useMemo(() => (tree ? flattenTeams(tree.roots) : []), [tree]);

  // Debounce search input.
  useEffect(() => {
    const timer = setTimeout(() => setDebouncedSearch(search), 300);
    return (): void => {
      clearTimeout(timer);
    };
  }, [search]);

  // Reset to first page when filters change.
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

  if (isLoading && people.length === 0) {
    return (
      <div className="space-y-3 pt-4">
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
      </div>
    );
  }

  if (isError) {
    return (
      <Alert variant="destructive" className="mt-4">
        {error?.message ?? "Failed to load people"}
      </Alert>
    );
  }

  return (
    <div className="space-y-4 pt-4">
      <div className="flex flex-wrap items-center gap-3">
        <Input
          placeholder="Search people..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
          className="max-w-xs"
        />
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

      <DataTable
        columns={columns}
        data={people}
        sorting={sorting}
        onSortingChange={setSorting}
        onRowClick={setSelectedPerson}
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

      {selectedPerson && (
        <PersonDetailDialog
          person={selectedPerson}
          teams={allTeams}
          open={!!selectedPerson}
          onOpenChange={(open) => {
            if (!open) setSelectedPerson(null);
          }}
        />
      )}
    </div>
  );
};

const PersonDetailDialog = ({
  person,
  teams,
  open,
  onOpenChange,
}: {
  person: Person;
  teams: Team[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const updatePerson = useUpdatePerson();
  const deactivate = useDeactivatePerson();
  const reactivate = useReactivatePerson();
  const assign = useAssignPersonToTeam();
  const removeFromTeam = useRemovePersonFromTeam();

  const [name, setName] = useState(person.name);
  const [email, setEmail] = useState(person.email ?? "");
  const [level, setLevel] = useState(person.level ?? "");
  const [teamId, setTeamId] = useState(person.teamId ?? "");

  useEffect(() => {
    setName(person.name);
    setEmail(person.email ?? "");
    setLevel(person.level ?? "");
    setTeamId(person.teamId ?? "");
  }, [person]);

  const handleSubmit = async (e: React.FormEvent): Promise<void> => {
    e.preventDefault();

    const mutations: Promise<unknown>[] = [];

    const nameChanged = name !== person.name;
    const emailChanged = email !== (person.email ?? "");
    const levelChanged = level !== (person.level ?? "");
    if (nameChanged || emailChanged || levelChanged) {
      mutations.push(
        updatePerson.mutateAsync({
          personId: person.id,
          name: nameChanged ? name : undefined,
          email: emailChanged ? email : undefined,
          level: levelChanged ? level : undefined,
        }),
      );
    }

    const teamChanged = teamId !== (person.teamId ?? "");
    if (teamChanged) {
      if (person.teamId) {
        mutations.push(removeFromTeam.mutateAsync({ personId: person.id, teamId: person.teamId }));
      }
      if (teamId) {
        mutations.push(assign.mutateAsync({ personId: person.id, teamId }));
      }
    }

    if (mutations.length === 0) {
      onOpenChange(false);
      return;
    }

    try {
      await Promise.all(mutations);
      onOpenChange(false);
    } catch (err) {
      toast.error(err instanceof Error ? err.message : "Failed to save changes");
    }
  };

  const handleToggleActive = (): void => {
    if (person.active) {
      deactivate.mutate(person.id, { onSuccess: () => onOpenChange(false) });
    } else {
      reactivate.mutate(person.id, { onSuccess: () => onOpenChange(false) });
    }
  };

  const isPending = updatePerson.isPending || assign.isPending || removeFromTeam.isPending;
  const mutationError = updatePerson.error ?? assign.error ?? removeFromTeam.error;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>{person.name}</DialogTitle>
            <DialogDescription>
              Edit details, team assignment, and status.
              {!person.active && " This person is currently inactive."}
            </DialogDescription>
          </DialogHeader>

          <div className="mt-4 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="person-name">Name</Label>
              <Input
                id="person-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                required
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="person-email">Email</Label>
              <Input
                id="person-email"
                type="email"
                value={email}
                onChange={(e) => setEmail(e.target.value)}
              />
            </div>
            <div className="space-y-2">
              <Label htmlFor="person-level">Level / Title</Label>
              <Input id="person-level" value={level} onChange={(e) => setLevel(e.target.value)} />
            </div>

            <Separator />

            <div className="space-y-2">
              <Label htmlFor="person-team">Team</Label>
              <Select value={teamId} onValueChange={(v) => v !== null && setTeamId(v)}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="No team">
                    {teams.find((t) => t.id === teamId)?.name ?? "No team"}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {teams.map((t) => (
                    <SelectItem key={t.id} value={t.id}>
                      {t.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {person.identities.length > 0 && (
              <>
                <Separator />
                <div className="space-y-2">
                  <Label>Platform Identities</Label>
                  <div className="space-y-1">
                    {person.identities.map((id) => (
                      <div
                        key={`${id.platform}-${id.username}`}
                        className="flex items-center justify-between rounded-md border px-3 py-1.5 text-sm"
                      >
                        <span className="font-medium capitalize">{id.platform}</span>
                        <span className="text-muted-foreground">{id.username}</span>
                      </div>
                    ))}
                  </div>
                </div>
              </>
            )}

            <Separator />

            <div className="flex items-center justify-between">
              <div>
                <p className="text-sm font-medium">{person.active ? "Deactivate" : "Reactivate"}</p>
                <p className="text-sm text-muted-foreground">
                  {person.active
                    ? "Remove this person from active reporting."
                    : "Restore this person to active status."}
                </p>
              </div>
              <Button
                type="button"
                variant={person.active ? "destructive" : "outline"}
                size="sm"
                onClick={handleToggleActive}
                disabled={deactivate.isPending || reactivate.isPending}
              >
                {person.active ? "Deactivate" : "Reactivate"}
              </Button>
            </div>

            {mutationError && (
              <Alert variant="destructive">
                {mutationError instanceof Error ? mutationError.message : "An error occurred"}
              </Alert>
            )}
          </div>

          <DialogFooter className="mt-4">
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={isPending || !name.trim()}>
              {isPending ? "Saving..." : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
