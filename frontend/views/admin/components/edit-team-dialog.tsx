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
import { useEffect, useState } from "react";

import type { Team } from "@ps/api/gen/prism/v1/org_pb";

import { useUpdateTeam } from "@/views/admin/hooks/use-admin";
import { useListPeople } from "@/views/teams/hooks/use-teams";

interface EditTeamDialogProps {
  team: Team;
  teams: Team[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export const EditTeamDialog = ({
  team,
  teams,
  open,
  onOpenChange,
}: EditTeamDialogProps): React.ReactElement => {
  const updateTeam = useUpdateTeam();
  const { data: people } = useListPeople();
  const [name, setName] = useState(team.name);
  const [leadId, setLeadId] = useState(team.leadId ?? "");
  const [parentTeamId, setParentTeamId] = useState(team.parentTeamId ?? "");
  const [githubTeamSlug, setGithubTeamSlug] = useState(team.githubTeamSlug ?? "");

  useEffect(() => {
    setName(team.name);
    setLeadId(team.leadId ?? "");
    setParentTeamId(team.parentTeamId ?? "");
    setGithubTeamSlug(team.githubTeamSlug ?? "");
  }, [team]);

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    updateTeam.mutate(
      {
        teamId: team.id,
        name: name !== team.name ? name : undefined,
        leadId: leadId && leadId !== team.leadId ? leadId : undefined,
        parentTeamId: parentTeamId && parentTeamId !== team.parentTeamId ? parentTeamId : undefined,
        githubTeamSlug:
          githubTeamSlug && githubTeamSlug !== team.githubTeamSlug ? githubTeamSlug : undefined,
      },
      {
        onSuccess: () => {
          onOpenChange(false);
        },
      },
    );
  };

  // Filter out the current team and its descendants from parent options.
  const validParents = teams.filter((t) => t.id !== team.id);

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>Edit Team</DialogTitle>
            <DialogDescription>
              Update team details for &ldquo;{team.name}&rdquo;.
            </DialogDescription>
          </DialogHeader>

          <div className="mt-4 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="team-name">Name</Label>
              <Input
                id="team-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                required
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="team-lead">Lead</Label>
              <Select value={leadId} onValueChange={(v) => v !== null && setLeadId(v)}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="Select a lead..." />
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
              <Label htmlFor="team-parent">Parent Team</Label>
              <Select value={parentTeamId} onValueChange={(v) => v !== null && setParentTeamId(v)}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="No parent" />
                </SelectTrigger>
                <SelectContent>
                  {validParents.map((t) => (
                    <SelectItem key={t.id} value={t.id}>
                      {t.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="github-slug">GitHub Team Slug</Label>
              <Input
                id="github-slug"
                value={githubTeamSlug}
                onChange={(e) => setGithubTeamSlug(e.target.value)}
                placeholder="e.g. my-team"
              />
            </div>

            {updateTeam.isError && (
              <Alert variant="destructive">
                {updateTeam.error instanceof Error
                  ? updateTeam.error.message
                  : "Failed to update team"}
              </Alert>
            )}
          </div>

          <DialogFooter className="mt-4">
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={updateTeam.isPending || !name.trim()}>
              {updateTeam.isPending ? "Saving..." : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
