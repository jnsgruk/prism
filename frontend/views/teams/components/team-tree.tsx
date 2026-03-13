import { Badge } from "@/components/ui/badge";
import { ChevronDown, ChevronRight, Users } from "lucide-react";
import { useState } from "react";

import type { Team } from "@ps/api/gen/prism/v1/org_pb";

import { teamTypeBadgeVariant, teamTypeLabel } from "@/views/teams/hooks/use-teams";

/** A single row in the team tree, with expand/collapse for children. */
const TeamTreeNode = ({
  team,
  depth,
  selectedTeamId,
  onSelect,
}: {
  team: Team;
  depth: number;
  selectedTeamId: string | null;
  onSelect: (teamId: string) => void;
}): React.ReactElement => {
  const [expanded, setExpanded] = useState(depth < 2);
  const hasChildren = team.children.length > 0;
  const isSelected = selectedTeamId === team.id;

  return (
    <>
      <button
        type="button"
        className={`flex w-full items-center gap-2 border-b px-4 py-2.5 text-left text-sm transition-colors hover:bg-muted/50 ${
          isSelected ? "bg-muted/50" : ""
        }`}
        style={{ paddingLeft: `${depth * 1.25 + 1}rem` }}
        onClick={() => onSelect(team.id)}
      >
        {hasChildren ? (
          <button
            type="button"
            className="shrink-0 rounded p-0.5 hover:bg-muted"
            onClick={(e) => {
              e.stopPropagation();
              setExpanded(!expanded);
            }}
          >
            {expanded ? (
              <ChevronDown className="size-3.5" />
            ) : (
              <ChevronRight className="size-3.5" />
            )}
          </button>
        ) : (
          <span className="w-5" />
        )}

        <span className="min-w-0 flex-1 truncate font-medium">{team.name}</span>

        <Badge variant={teamTypeBadgeVariant(team.teamType)} className="shrink-0 text-[10px]">
          {teamTypeLabel(team.teamType)}
        </Badge>

        {team.leadName && (
          <span className="hidden shrink-0 text-xs text-muted-foreground sm:inline">
            {team.leadName}
          </span>
        )}

        <span className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
          <Users className="size-3" />
          {team.totalMemberCount > 0 ? team.totalMemberCount : team.memberCount}
        </span>
      </button>

      {expanded &&
        hasChildren &&
        team.children.map((child) => (
          <TeamTreeNode
            key={child.id}
            team={child}
            depth={depth + 1}
            selectedTeamId={selectedTeamId}
            onSelect={onSelect}
          />
        ))}
    </>
  );
};

export const TeamTree = ({
  roots,
  selectedTeamId,
  onSelect,
}: {
  roots: Team[];
  selectedTeamId: string | null;
  onSelect: (teamId: string) => void;
}): React.ReactElement => {
  if (roots.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
        <Users className="mb-3 size-10 text-muted-foreground" />
        <p className="mb-1 font-medium">No teams yet</p>
        <p className="text-sm text-muted-foreground">Import a directory file to get started.</p>
      </div>
    );
  }

  return (
    <div className="overflow-hidden rounded-lg border">
      {roots.map((root) => (
        <TeamTreeNode
          key={root.id}
          team={root}
          depth={0}
          selectedTeamId={selectedTeamId}
          onSelect={onSelect}
        />
      ))}
    </div>
  );
};
