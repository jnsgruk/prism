import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { Plus } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useSearchParams } from "react-router";

import type { Team } from "@ps/api/gen/prism/v1/org_pb";

import { TeamDetailPanel } from "@/views/teams/components/team-detail-panel";
import { TeamTree } from "@/views/teams/components/team-tree";
import { useGetTeamTree } from "@/views/teams/hooks/use-teams";
import { useDeleteTeam } from "@/views/admin/hooks/use-admin";
import { AddTeamDialog } from "@/views/admin/components/add-team-dialog";
import { EditTeamDialog } from "@/views/admin/components/edit-team-dialog";

/** Recursively flatten a tree of teams into a flat list. */
const flattenTeams = (teams: Team[]): Team[] =>
  teams.flatMap((t) => [t, ...flattenTeams(t.children)]);

export const TeamsTab = (): React.ReactElement => {
  const { data: tree, isLoading } = useGetTeamTree();
  const deleteTeam = useDeleteTeam();
  const [searchParams, setSearchParams] = useSearchParams();
  const selectedTeamId = searchParams.get("team");
  const [editingTeam, setEditingTeam] = useState<Team | null>(null);
  const [addDialogOpen, setAddDialogOpen] = useState(false);

  const allTeams = useMemo(() => (tree ? flattenTeams(tree.roots) : []), [tree]);

  const setSelectedTeamId = useCallback(
    (id: string | null) => {
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (id) next.set("team", id);
          else next.delete("team");
          return next;
        },
        { replace: true },
      );
    },
    [setSearchParams],
  );

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
        <Button onClick={() => setAddDialogOpen(true)}>
          <Plus className="size-4" />
          Add Team
        </Button>
      </div>

      {isLoading && <p className="text-sm text-muted-foreground">Loading...</p>}

      {tree && (
        <TeamTree
          roots={tree.roots}
          selectedTeamId={selectedTeamId}
          onSelect={setSelectedTeamId}
          onEdit={setEditingTeam}
          onDelete={handleDelete}
        />
      )}

      <Sheet
        open={!!selectedTeamId}
        onOpenChange={(open) => {
          if (!open) setSelectedTeamId(null);
        }}
      >
        <SheetContent className="overflow-y-auto sm:max-w-md">
          <SheetHeader className="sr-only">
            <SheetTitle>Team Details</SheetTitle>
            <SheetDescription>
              View and manage team details, members, and GitHub team mappings.
            </SheetDescription>
          </SheetHeader>
          {selectedTeamId && <TeamDetailPanel teamId={selectedTeamId} />}
        </SheetContent>
      </Sheet>

      <AddTeamDialog teams={allTeams} open={addDialogOpen} onOpenChange={setAddDialogOpen} />

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
