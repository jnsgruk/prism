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
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Pencil, UserMinus, UserPlus, UserX, UserCheck } from "lucide-react";
import { useMemo, useState } from "react";

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
  const [editingPerson, setEditingPerson] = useState<Person | null>(null);
  const [assigningPerson, setAssigningPerson] = useState<Person | null>(null);

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
      <div className="flex items-center gap-3">
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
            <TableHead>Email</TableHead>
            <TableHead>Level</TableHead>
            <TableHead>Team</TableHead>
            <TableHead>Status</TableHead>
            <TableHead className="w-[120px]">Actions</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {filtered.length === 0 && (
            <TableRow>
              <TableCell colSpan={6} className="text-center text-muted-foreground">
                {search ? "No people match your search." : "No people found."}
              </TableCell>
            </TableRow>
          )}
          {filtered.map((person) => (
            <PersonRow
              key={person.id}
              person={person}
              onEdit={() => setEditingPerson(person)}
              onAssign={() => setAssigningPerson(person)}
            />
          ))}
        </TableBody>
      </Table>

      {editingPerson && (
        <EditPersonDialog
          person={editingPerson}
          open={!!editingPerson}
          onOpenChange={(open) => {
            if (!open) setEditingPerson(null);
          }}
        />
      )}

      {assigningPerson && (
        <AssignTeamDialog
          person={assigningPerson}
          teams={allTeams}
          open={!!assigningPerson}
          onOpenChange={(open) => {
            if (!open) setAssigningPerson(null);
          }}
        />
      )}
    </div>
  );
};

const PersonRow = ({
  person,
  onEdit,
  onAssign,
}: {
  person: Person;
  onEdit: () => void;
  onAssign: () => void;
}): React.ReactElement => {
  const deactivate = useDeactivatePerson();
  const reactivate = useReactivatePerson();
  const removeFromTeam = useRemovePersonFromTeam();

  return (
    <TableRow>
      <TableCell className="font-medium">{person.name}</TableCell>
      <TableCell className="text-muted-foreground">{person.email ?? "\u2014"}</TableCell>
      <TableCell className="text-muted-foreground">{person.level ?? "\u2014"}</TableCell>
      <TableCell>
        {person.teamName ? (
          <Badge variant="secondary">{person.teamName}</Badge>
        ) : (
          <span className="text-muted-foreground">\u2014</span>
        )}
      </TableCell>
      <TableCell>
        <Badge variant={person.active ? "outline" : "destructive"}>
          {person.active ? "Active" : "Inactive"}
        </Badge>
      </TableCell>
      <TableCell>
        <div className="flex items-center gap-1">
          <Button variant="ghost" size="icon-sm" title="Edit person" onClick={onEdit}>
            <Pencil className="size-3.5" />
          </Button>
          <Button variant="ghost" size="icon-sm" title="Assign to team" onClick={onAssign}>
            <UserPlus className="size-3.5" />
          </Button>
          {person.teamId && (
            <Button
              variant="ghost"
              size="icon-sm"
              title="Remove from team"
              onClick={() => removeFromTeam.mutate({ personId: person.id, teamId: person.teamId! })}
            >
              <UserMinus className="size-3.5" />
            </Button>
          )}
          {person.active ? (
            <Button
              variant="ghost"
              size="icon-sm"
              title="Deactivate"
              className="hover:text-destructive"
              onClick={() => deactivate.mutate(person.id)}
            >
              <UserX className="size-3.5" />
            </Button>
          ) : (
            <Button
              variant="ghost"
              size="icon-sm"
              title="Reactivate"
              onClick={() => reactivate.mutate(person.id)}
            >
              <UserCheck className="size-3.5" />
            </Button>
          )}
        </div>
      </TableCell>
    </TableRow>
  );
};

const EditPersonDialog = ({
  person,
  open,
  onOpenChange,
}: {
  person: Person;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const updatePerson = useUpdatePerson();
  const [name, setName] = useState(person.name);
  const [email, setEmail] = useState(person.email ?? "");
  const [level, setLevel] = useState(person.level ?? "");

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    updatePerson.mutate(
      {
        personId: person.id,
        name: name !== person.name ? name : undefined,
        email: email && email !== (person.email ?? "") ? email : undefined,
        level: level && level !== (person.level ?? "") ? level : undefined,
      },
      { onSuccess: () => onOpenChange(false) },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>Edit Person</DialogTitle>
            <DialogDescription>Update details for {person.name}.</DialogDescription>
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
            {updatePerson.isError && (
              <Alert variant="destructive">{updatePerson.error.message}</Alert>
            )}
          </div>
          <DialogFooter className="mt-4">
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={updatePerson.isPending || !name.trim()}>
              {updatePerson.isPending ? "Saving..." : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};

const AssignTeamDialog = ({
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
  const assign = useAssignPersonToTeam();
  const [teamId, setTeamId] = useState("");

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!teamId) return;
    assign.mutate({ personId: person.id, teamId }, { onSuccess: () => onOpenChange(false) });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>Assign to Team</DialogTitle>
            <DialogDescription>
              Choose a team for {person.name}.
              {person.teamName && ` Currently on "${person.teamName}".`}
            </DialogDescription>
          </DialogHeader>
          <div className="mt-4 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="assign-team">Team</Label>
              <Select value={teamId} onValueChange={(v) => v !== null && setTeamId(v)}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="Select a team..." />
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
            {assign.isError && <Alert variant="destructive">{assign.error.message}</Alert>}
          </div>
          <DialogFooter className="mt-4">
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={assign.isPending || !teamId}>
              {assign.isPending ? "Assigning..." : "Assign"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
