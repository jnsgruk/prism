"use client";

import { PageHeader } from "@/components/page-header";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { AlertCircle, ChevronRight, Users } from "lucide-react";
import { useState } from "react";

import { cn } from "@ps/cn";

import { ImportDirectoryDialog } from "@/views/teams/components/import-directory-dialog";
import { TeamDetailPanel } from "@/views/teams/components/team-detail-panel";
import { useListTeams } from "@/views/teams/hooks/use-teams";

const TeamsPage = (): React.ReactElement => {
  const [selectedTeamId, setSelectedTeamId] = useState<string | null>(null);
  const { data: teams, isLoading, error } = useListTeams();

  return (
    <>
      <PageHeader
        title="Teams"
        description="Manage your organization structure and team memberships"
        actions={<ImportDirectoryDialog />}
      />
      <div className="flex-1 p-6">
        {isLoading && <p className="text-sm text-muted-foreground">Loading teams...</p>}

        {error && (
          <Alert variant="destructive">
            <AlertCircle className="size-4" />
            Failed to load teams.
          </Alert>
        )}

        {teams && teams.length === 0 && (
          <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
            <Users className="mb-3 size-10 text-muted-foreground" />
            <p className="mb-1 font-medium">No teams yet</p>
            <p className="text-sm text-muted-foreground">Import a directory file to get started.</p>
          </div>
        )}

        {teams && teams.length > 0 && (
          <div className="grid gap-6 lg:grid-cols-2">
            <div className="space-y-2">
              {teams.map((team) => (
                <button
                  key={team.id}
                  onClick={() => setSelectedTeamId(team.id)}
                  className={cn(
                    "flex w-full items-center justify-between rounded-lg border px-4 py-3 text-left transition-colors hover:bg-muted/50",
                    selectedTeamId === team.id && "border-primary bg-muted/50",
                  )}
                >
                  <div>
                    <p className="text-sm font-medium">{team.name}</p>
                    <p className="text-xs text-muted-foreground">{team.orgName}</p>
                  </div>
                  <div className="flex items-center gap-2">
                    <Badge variant="secondary">
                      {team.memberCount} {team.memberCount === 1 ? "member" : "members"}
                    </Badge>
                    <ChevronRight className="size-4 text-muted-foreground" />
                  </div>
                </button>
              ))}
            </div>

            <div>
              {selectedTeamId ? (
                <TeamDetailPanel teamId={selectedTeamId} onClose={() => setSelectedTeamId(null)} />
              ) : (
                <div className="flex h-full items-center justify-center rounded-lg border-2 border-dashed p-12">
                  <p className="text-sm text-muted-foreground">
                    Select a team to view its members.
                  </p>
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </>
  );
};

export default TeamsPage;
