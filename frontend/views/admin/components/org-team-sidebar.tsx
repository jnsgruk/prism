import { Button } from "@/components/ui/button";
import { useGetTeamTree } from "@/lib/hooks/use-org";
import { useListUnassignedPeople } from "@/views/admin/hooks/use-admin";
import { TeamTree } from "@/views/teams/components/team-tree";
import { Plus, Users, UserX } from "lucide-react";
import { useMemo } from "react";

import type { Team } from "@ps/api/gen/canonical/prism/v1/org_pb";

/** Sentinel value for the "Unassigned" pseudo-node in URL state. */
export const UNASSIGNED_TEAM_ID = "__unassigned__";

/** Count all people across the tree (sum of root totalMemberCount values). */
const countAllMembers = (roots: Team[]): number =>
  roots.reduce((sum, r) => sum + (r.totalMemberCount > 0 ? r.totalMemberCount : r.memberCount), 0);

export const OrgTeamSidebar = ({
  selectedTeamId,
  onSelectTeam,
  onAddTeam,
}: {
  selectedTeamId: string | null;
  onSelectTeam: (id: string | null) => void;
  onAddTeam: () => void;
}): React.ReactElement => {
  const { data: tree, isLoading } = useGetTeamTree();
  const { data: unassigned } = useListUnassignedPeople();

  const roots = useMemo(() => tree?.roots ?? [], [tree]);
  const totalMembers = useMemo(() => countAllMembers(roots), [roots]);
  const unassignedCount = unassigned?.length ?? 0;
  const allCount = totalMembers + unassignedCount;

  const isAllSelected = selectedTeamId === null;
  const isUnassignedSelected = selectedTeamId === UNASSIGNED_TEAM_ID;

  return (
    <div className="flex h-full flex-col overflow-hidden rounded-lg border">
      {/* Header with Add Team button */}
      <div className="flex items-center justify-between border-b px-3 py-2">
        <span className="text-sm font-medium">Teams</span>
        <Button variant="ghost" size="icon-sm" title="Add team" onClick={onAddTeam}>
          <Plus className="size-3.5" />
        </Button>
      </div>

      {/* "All people" pseudo-node */}
      <button
        type="button"
        className={`flex w-full items-center gap-2 border-b px-3 py-2 text-left text-sm transition-colors hover:bg-muted/50 ${
          isAllSelected ? "bg-muted/50 font-medium" : ""
        }`}
        onClick={() => onSelectTeam(null)}
      >
        <Users className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate">All people</span>
        <span className="shrink-0 text-xs text-muted-foreground">{allCount}</span>
      </button>

      {/* Team tree */}
      <div className="min-h-0 flex-1 overflow-y-auto">
        {isLoading && <p className="px-3 py-2 text-sm text-muted-foreground">Loading...</p>}
        {!isLoading && roots.length > 0 && (
          <TeamTree
            roots={roots}
            selectedTeamId={isAllSelected || isUnassignedSelected ? null : selectedTeamId}
            onSelect={onSelectTeam}
          />
        )}
        {!isLoading && roots.length === 0 && (
          <p className="px-3 py-4 text-center text-sm text-muted-foreground">
            No teams yet. Add a team or import a directory.
          </p>
        )}
      </div>

      {/* "Unassigned" pseudo-node */}
      <button
        type="button"
        className={`flex w-full items-center gap-2 border-t px-3 py-2 text-left text-sm transition-colors hover:bg-muted/50 ${
          isUnassignedSelected ? "bg-muted/50 font-medium" : ""
        }`}
        onClick={() => onSelectTeam(UNASSIGNED_TEAM_ID)}
      >
        <UserX className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate">Unassigned</span>
        <span className="shrink-0 text-xs text-muted-foreground">{unassignedCount}</span>
      </button>
    </div>
  );
};
