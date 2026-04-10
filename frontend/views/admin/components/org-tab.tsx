import { ConfirmDialog } from "@/components/confirm-dialog";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useGetTeamTree } from "@/lib/hooks/use-org";
import { AddTeamDialog } from "@/views/admin/components/add-team-dialog";
import { EditTeamDialog } from "@/views/admin/components/edit-team-dialog";
import { ImportDirectoryDialog } from "@/views/admin/components/import-directory-dialog";
import { ImportJiraUsersDialog } from "@/views/admin/components/import-jira-users-dialog";
import { OrgPeoplePanel } from "@/views/admin/components/org-people-panel";
import { OrgTeamSidebar } from "@/views/admin/components/org-team-sidebar";
import { PersonDetailDialog } from "@/views/admin/components/person-detail-dialog";
import { useDeleteTeam } from "@/views/admin/hooks/use-admin";
import { flattenTeams } from "@/views/admin/lib/team-utils";
import { ChevronDown, FileSpreadsheet, FileUp, Plus, Users } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useSearchParams } from "react-router";

import type { Person, Team } from "@ps/api/gen/canonical/prism/v1/org_pb";

export const OrgTab = (): React.ReactElement => {
  const { data: tree } = useGetTeamTree();
  const deleteTeam = useDeleteTeam();
  const [searchParams, setSearchParams] = useSearchParams();

  // Team selection from URL.
  const selectedTeamId = searchParams.get("team") || null;

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

  // Dialog state.
  const [addDialogOpen, setAddDialogOpen] = useState(false);
  const [importDirOpen, setImportDirOpen] = useState(false);
  const [importJiraOpen, setImportJiraOpen] = useState(false);
  const [editingTeam, setEditingTeam] = useState<Team | null>(null);
  const [deletingTeam, setDeletingTeam] = useState<Team | null>(null);
  const [selectedPerson, setSelectedPerson] = useState<Person | null>(null);

  const allTeams = useMemo(() => (tree ? flattenTeams(tree.roots) : []), [tree]);

  return (
    <div className="space-y-4 pt-4">
      {/* Top action bar */}
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">Manage your organisation's teams and people.</p>
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button />}>
            <Plus className="size-4" />
            Add
            <ChevronDown className="size-3.5" />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end" className="w-48">
            <DropdownMenuItem onClick={() => setAddDialogOpen(true)}>
              <Users className="size-4" />
              Add Team
            </DropdownMenuItem>
            <DropdownMenuSeparator />
            <DropdownMenuItem onClick={() => setImportDirOpen(true)}>
              <FileUp className="size-4" />
              Import Directory
            </DropdownMenuItem>
            <DropdownMenuItem onClick={() => setImportJiraOpen(true)}>
              <FileSpreadsheet className="size-4" />
              Import Jira Users
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      {/* Master-detail layout */}
      <div className="flex gap-4">
        {/* Sidebar — hidden on mobile */}
        <div className="hidden w-80 shrink-0 md:block">
          <div className="sticky top-0 max-h-[calc(100vh-12rem)]">
            <OrgTeamSidebar
              selectedTeamId={selectedTeamId}
              onSelectTeam={setSelectedTeamId}
              onAddTeam={() => setAddDialogOpen(true)}
            />
          </div>
        </div>

        {/* People panel */}
        <div className="min-w-0 flex-1">
          <OrgPeoplePanel
            teamId={selectedTeamId}
            onSelectTeam={setSelectedTeamId}
            onSelectPerson={setSelectedPerson}
            onEditTeam={setEditingTeam}
            onDeleteTeam={setDeletingTeam}
          />
        </div>
      </div>

      {/* Person detail dialog */}
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

      {/* Add/Edit/Delete team dialogs + imports */}
      <AddTeamDialog teams={allTeams} open={addDialogOpen} onOpenChange={setAddDialogOpen} />
      <ImportDirectoryDialog open={importDirOpen} onOpenChange={setImportDirOpen} />
      <ImportJiraUsersDialog open={importJiraOpen} onOpenChange={setImportJiraOpen} />

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

      {deletingTeam && (
        <ConfirmDialog
          open={!!deletingTeam}
          onOpenChange={(open) => {
            if (!open) setDeletingTeam(null);
          }}
          title={`Delete "${deletingTeam.name}"?`}
          description="This will permanently delete the team and all sub-teams. This action cannot be undone."
          confirmLabel="Delete"
          onConfirm={() => deleteTeam.mutate(deletingTeam.id)}
        />
      )}
    </div>
  );
};
