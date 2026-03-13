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
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { useEffect, useMemo, useState } from "react";

import type { Person, Team } from "@ps/api/gen/prism/v1/org_pb";

import { useListPeople, useGetTeamTree } from "@/views/teams/hooks/use-teams";
import {
  useUpdatePerson,
  useDeactivatePerson,
  useReactivatePerson,
  useAssignPersonToTeam,
  useRemovePersonFromTeam,
} from "@/views/admin/hooks/use-admin";

type Filter = "all" | "unassigned" | "inactive";

/** Recursively flatten tree into a flat team list. */
const flattenTeams = (teams: Team[]): Team[] =>
  teams.flatMap((t) => [t, ...flattenTeams(t.children)]);

export const PeopleTab = (): React.ReactElement => {
  const { data: people, isLoading, isError, error } = useListPeople();
  const { data: tree } = useGetTeamTree();
  const [filter, setFilter] = useState<Filter>("all");
  const [search, setSearch] = useState("");
  const [selectedPerson, setSelectedPerson] = useState<Person | null>(null);

  const allTeams = useMemo(() => (tree ? flattenTeams(tree.roots) : []), [tree]);

  const filtered = useMemo(() => {
    if (!people) return [];
    let result = people;
    if (filter === "unassigned") result = result.filter((p) => p.active && !p.teamId);
    else if (filter === "inactive") result = result.filter((p) => !p.active);
    if (search.trim()) {
      const q = search.toLowerCase();
      result = result.filter(
        (p) =>
          p.name.toLowerCase().includes(q) ||
          p.email?.toLowerCase().includes(q) ||
          p.teamName?.toLowerCase().includes(q),
      );
    }
    return result;
  }, [people, filter, search]);

  const unassignedCount = useMemo(
    () => people?.filter((p) => p.active && !p.teamId).length ?? 0,
    [people],
  );
  const inactiveCount = useMemo(() => people?.filter((p) => !p.active).length ?? 0, [people]);

  if (isLoading) {
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
            All ({people?.length ?? 0})
          </Button>
          <Button
            variant={filter === "unassigned" ? "default" : "outline"}
            size="sm"
            onClick={() => setFilter("unassigned")}
          >
            Unassigned ({unassignedCount})
          </Button>
          <Button
            variant={filter === "inactive" ? "default" : "outline"}
            size="sm"
            onClick={() => setFilter("inactive")}
          >
            Inactive ({inactiveCount})
          </Button>
        </div>
      </div>

      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Name</TableHead>
            <TableHead>Team</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {filtered.length === 0 && (
            <TableRow>
              <TableCell colSpan={2} className="text-center text-muted-foreground">
                {search ? "No people match your search." : "No people found."}
              </TableCell>
            </TableRow>
          )}
          {filtered.map((person) => (
            <TableRow
              key={person.id}
              className="cursor-pointer"
              onClick={() => setSelectedPerson(person)}
            >
              <TableCell>
                <div className="flex items-center gap-2">
                  <span className="font-medium">{person.name}</span>
                  {!person.active && <Badge variant="destructive">Inactive</Badge>}
                </div>
              </TableCell>
              <TableCell>
                {person.teamName ? (
                  <Badge variant="secondary">{person.teamName}</Badge>
                ) : (
                  <span className="text-muted-foreground">&mdash;</span>
                )}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>

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

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();

    // Update person fields if changed.
    const nameChanged = name !== person.name;
    const emailChanged = email !== (person.email ?? "");
    const levelChanged = level !== (person.level ?? "");
    if (nameChanged || emailChanged || levelChanged) {
      updatePerson.mutate({
        personId: person.id,
        name: nameChanged ? name : undefined,
        email: emailChanged ? email : undefined,
        level: levelChanged ? level : undefined,
      });
    }

    // Update team assignment if changed.
    const teamChanged = teamId !== (person.teamId ?? "");
    if (teamChanged) {
      if (person.teamId) {
        removeFromTeam.mutate({ personId: person.id, teamId: person.teamId });
      }
      if (teamId) {
        assign.mutate({ personId: person.id, teamId });
      }
    }

    onOpenChange(false);
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
