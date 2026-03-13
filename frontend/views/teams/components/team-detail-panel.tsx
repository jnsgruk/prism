import { useState } from "react";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Separator } from "@/components/ui/separator";
import { GitBranch, Plus, X } from "lucide-react";

import { useUnassignGithubTeam } from "@/views/admin/hooks/use-admin";
import {
  teamTypeBadgeVariant,
  teamTypeLabel,
  useGetTeam,
  useListTeamGithubTeams,
} from "@/views/teams/hooks/use-teams";

import { GithubTeamPickerDialog } from "./github-team-picker-dialog";
import { TeamMappingSuggestions } from "./team-mapping-suggestions";

export const TeamDetailPanel = ({
  teamId,
  onClose,
}: {
  teamId: string;
  onClose: () => void;
}): React.ReactElement => {
  const { data, isLoading, error } = useGetTeam(teamId);
  const { data: githubTeams } = useListTeamGithubTeams(teamId);
  const unassign = useUnassignGithubTeam();
  const [pickerOpen, setPickerOpen] = useState(false);

  if (isLoading) {
    return (
      <Card>
        <CardContent className="p-6">
          <p className="text-sm text-muted-foreground">Loading team details...</p>
        </CardContent>
      </Card>
    );
  }

  if (error || !data?.team) {
    return (
      <Card>
        <CardContent className="p-6">
          <Alert variant="destructive">Failed to load team details.</Alert>
        </CardContent>
      </Card>
    );
  }

  const { team, members } = data;

  return (
    <Card className="overflow-hidden">
      <CardHeader>
        <div className="flex items-center gap-2">
          <CardTitle className="truncate">{team.name}</CardTitle>
          <Badge variant={teamTypeBadgeVariant(team.teamType)}>
            {teamTypeLabel(team.teamType)}
          </Badge>
        </div>
        {team.leadName && (
          <p className="truncate text-sm text-muted-foreground">Lead: {team.leadName}</p>
        )}
        <CardAction>
          <Button variant="ghost" size="icon" onClick={onClose}>
            <X className="size-4" />
            <span className="sr-only">Close</span>
          </Button>
        </CardAction>
      </CardHeader>
      <CardContent className="space-y-6">
        {/* Members section */}
        <div>
          <h3 className="mb-3 text-sm font-medium">Members ({members.length})</h3>
          {members.length === 0 ? (
            <p className="text-sm text-muted-foreground">No members in this team.</p>
          ) : (
            <div className="space-y-2">
              {members.map((person) => (
                <div
                  key={person.id}
                  className="flex flex-wrap items-center justify-between gap-2 rounded border px-4 py-3"
                >
                  <div className="min-w-0">
                    <p className="truncate text-sm font-medium">{person.name}</p>
                    {person.email && (
                      <p className="truncate text-xs text-muted-foreground">{person.email}</p>
                    )}
                  </div>
                  <div className="flex flex-wrap gap-1">
                    {person.identities.map((id) => (
                      <Badge key={`${id.platform}-${id.username}`} variant="secondary">
                        {id.platform}
                      </Badge>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>

        <Separator />

        <TeamMappingSuggestions teamId={teamId} />

        {/* GitHub teams section */}
        <div>
          <div className="mb-3 flex items-center justify-between">
            <h3 className="text-sm font-medium">GitHub Teams ({githubTeams?.length ?? 0})</h3>
            <Button variant="outline" size="sm" onClick={() => setPickerOpen(true)}>
              <Plus className="mr-1 size-3" />
              Link
            </Button>
          </div>
          {!githubTeams || githubTeams.length === 0 ? (
            <p className="text-sm text-muted-foreground">
              No GitHub teams linked. Link a team to scope ingestion.
            </p>
          ) : (
            <div className="space-y-2">
              {githubTeams.map((gt) => (
                <div
                  key={gt.id}
                  className="flex items-center justify-between gap-2 rounded border px-4 py-3"
                >
                  <div className="flex min-w-0 items-center gap-2">
                    <GitBranch className="size-4 shrink-0 text-muted-foreground" />
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium">{gt.name}</p>
                      <p className="truncate text-xs text-muted-foreground">
                        {gt.githubOrg}/{gt.slug}
                      </p>
                    </div>
                  </div>
                  <div className="flex items-center gap-2">
                    <Badge variant="secondary">{Number(gt.memberCount)} members</Badge>
                    <Badge variant="secondary">{Number(gt.repoCount)} repos</Badge>
                    <Button
                      variant="ghost"
                      size="icon"
                      className="size-7"
                      onClick={() => unassign.mutate({ teamId, githubTeamId: gt.id })}
                    >
                      <X className="size-3" />
                      <span className="sr-only">Unlink</span>
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      </CardContent>

      <GithubTeamPickerDialog
        teamId={teamId}
        open={pickerOpen}
        onOpenChange={setPickerOpen}
        alreadyAssigned={githubTeams?.map((t) => t.id) ?? []}
      />
    </Card>
  );
};
