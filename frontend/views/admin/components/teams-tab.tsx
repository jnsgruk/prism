import { Button } from "@/components/ui/button";
import { Pencil, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";

import type { Team } from "@ps/api/gen/prism/v1/org_pb";

import { TeamTree } from "@/views/teams/components/team-tree";
import { useGetTeamTree } from "@/views/teams/hooks/use-teams";
import { useDeleteTeam } from "@/views/admin/hooks/use-admin";
import { ImportDirectoryDialog } from "@/views/admin/components/import-directory-dialog";
import { EditTeamDialog } from "@/views/admin/components/edit-team-dialog";

/** Recursively flatten a tree of teams into a flat list. */
const flattenTeams = (teams: Team[]): Team[] =>
  teams.flatMap((t) => [t, ...flattenTeams(t.children)]);

export const TeamsTab = (): React.ReactElement => {
  const { data: tree, isLoading } = useGetTeamTree();
  const deleteTeam = useDeleteTeam();
  const [selectedTeamId, setSelectedTeamId] = useState<string | null>(null);
  const [editingTeam, setEditingTeam] = useState<Team | null>(null);

  const allTeams = useMemo(() => (tree ? flattenTeams(tree.roots) : []), [tree]);

  const handleDelete = (team: Team): void => {
    if (confirm(`Delete team "${team.name}" and all sub-teams?`)) {
      deleteTeam.mutate(team.id);
    }
  };

  return (
    <div className="space-y-4 pt-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          Manage your organisation hierarchy. Import a directory or create teams manually.
        </p>
        <div className="flex items-center gap-2">
          <ImportDirectoryDialog />
        </div>
      </div>

      {isLoading && <p className="text-sm text-muted-foreground">Loading...</p>}

      {tree && (
        <TeamTree
          roots={tree.roots}
          selectedTeamId={selectedTeamId}
          onSelect={setSelectedTeamId}
          renderActions={(team) => (
            <>
              <Button
                variant="ghost"
                size="icon-sm"
                title="Edit team"
                onClick={() => setEditingTeam(team)}
              >
                <Pencil className="size-3.5" />
              </Button>
              <Button
                variant="ghost"
                size="icon-sm"
                title="Delete team"
                className="hover:text-destructive"
                onClick={() => handleDelete(team)}
              >
                <Trash2 className="size-3.5" />
              </Button>
            </>
          )}
        />
      )}

      {editingTeam && (
        <EditTeamDialog
          team={editingTeam}
          teams={allTeams}
          open={!!editingTeam}
          onOpenChange={(open) => {
            if (!open) setEditingTeam(null);
          }}
        />
      )}
    </div>
  );
};
