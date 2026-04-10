import { Alert } from "@/components/ui/alert";
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { platformLabel } from "@/lib/proto-display";
import {
  useUpdatePerson,
  useDeactivatePerson,
  useReactivatePerson,
  useAssignPersonToTeam,
  useRemovePersonFromTeam,
} from "@/views/admin/hooks/use-admin";
import { useEffect, useState } from "react";
import { toast } from "sonner";

import type { Person, Team } from "@ps/api/gen/canonical/prism/v1/org_pb";

export const PersonDetailDialog = ({
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
              <Input id="person-name" value={name} onChange={(e) => setName(e.target.value)} required />
            </div>
            <div className="space-y-2">
              <Label htmlFor="person-email">Email</Label>
              <Input id="person-email" type="email" value={email} onChange={(e) => setEmail(e.target.value)} />
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
                        <span className="font-medium">{platformLabel(id.platform, id.platformInstance)}</span>
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
