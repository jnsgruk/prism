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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useState } from "react";

import type { Team } from "@ps/api/gen/prism/v1/org_pb";
import { TeamType } from "@ps/api/gen/prism/v1/org_pb";

import { useCreateTeam } from "@/views/admin/hooks/use-admin";
import { useListPeople } from "@/views/teams/hooks/use-teams";

const teamTypeOptions = [
  { value: String(TeamType.GROUP), label: "Group" },
  { value: String(TeamType.TEAM), label: "Team" },
  { value: String(TeamType.SQUAD), label: "Squad" },
];

interface AddTeamDialogProps {
  teams: Team[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export const AddTeamDialog = ({
  teams,
  open,
  onOpenChange,
}: AddTeamDialogProps): React.ReactElement => {
  const createTeam = useCreateTeam();
  const { data: people } = useListPeople();
  const [name, setName] = useState("");
  const [teamType, setTeamType] = useState(String(TeamType.TEAM));
  const [leadId, setLeadId] = useState("");
  const [parentTeamId, setParentTeamId] = useState("");

  const reset = (): void => {
    setName("");
    setTeamType(String(TeamType.TEAM));
    setLeadId("");
    setParentTeamId("");
  };

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    createTeam.mutate(
      {
        name,
        teamType: Number(teamType) as TeamType,
        orgName: teams[0]?.orgName ?? "",
        leadId: leadId || undefined,
        parentTeamId: parentTeamId || undefined,
      },
      {
        onSuccess: () => {
          reset();
          onOpenChange(false);
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>Add Team</DialogTitle>
            <DialogDescription>Create a new team in the organisation hierarchy.</DialogDescription>
          </DialogHeader>

          <div className="mt-4 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="new-team-name">Name</Label>
              <Input
                id="new-team-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. Platform Team"
                required
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="new-team-type">Type</Label>
              <Select value={teamType} onValueChange={(v) => v !== null && setTeamType(v)}>
                <SelectTrigger className="w-full">
                  <SelectValue>
                    {teamTypeOptions.find((o) => o.value === teamType)?.label ?? "Select type..."}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {teamTypeOptions.map((o) => (
                    <SelectItem key={o.value} value={o.value}>
                      {o.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="new-team-lead">Lead</Label>
              <Select value={leadId} onValueChange={(v) => v !== null && setLeadId(v)}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="Select a lead...">
                    {people?.find((p) => p.id === leadId)?.name ?? "Select a lead..."}
                  </SelectValue>
                </SelectTrigger>
                <SelectContent>
                  {people?.map((p) => (
                    <SelectItem key={p.id} value={p.id}>
                      {p.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="new-team-parent">Parent Team</Label>
              <Select value={parentTeamId} onValueChange={(v) => v !== null && setParentTeamId(v)}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="No parent">
                    {teams.find((t) => t.id === parentTeamId)?.name ?? "No parent"}
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

            {createTeam.isError && (
              <Alert variant="destructive">
                {createTeam.error instanceof Error
                  ? createTeam.error.message
                  : "Failed to create team"}
              </Alert>
            )}
          </div>

          <DialogFooter className="mt-4">
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={createTeam.isPending || !name.trim()}>
              {createTeam.isPending ? "Creating..." : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
