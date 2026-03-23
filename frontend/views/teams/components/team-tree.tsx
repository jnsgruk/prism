import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { Input } from "@/components/ui/input";
import {
  ChevronDown,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  Ellipsis,
  Pencil,
  Search,
  Trash2,
  Users,
} from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";

import type { Team } from "@ps/api/gen/canonical/prism/v1/org_pb";

import { teamTypeBadgeVariant, teamTypeLabel } from "@/views/teams/hooks/use-teams";

/** Collect all team IDs in a tree. */
const collectIds = (teams: Team[]): Set<string> => {
  const ids = new Set<string>();
  const walk = (nodes: Team[]): void => {
    for (const t of nodes) {
      ids.add(t.id);
      walk(t.children);
    }
  };
  walk(teams);
  return ids;
};

/** Return IDs of all ancestors of teams matching the filter. */
const findMatchingAncestors = (teams: Team[], filter: string): Set<string> => {
  const ids = new Set<string>();
  const lowerFilter = filter.toLowerCase();
  const walk = (nodes: Team[]): boolean => {
    let anyMatch = false;
    for (const t of nodes) {
      const childMatch = walk(t.children);
      const selfMatch = t.name.toLowerCase().includes(lowerFilter);
      if (selfMatch || childMatch) {
        ids.add(t.id);
        anyMatch = true;
      }
    }
    return anyMatch;
  };
  walk(teams);
  return ids;
};

/** A single row in the team tree, with expand/collapse for children. */
const TeamTreeNode = ({
  team,
  depth,
  expandedIds,
  toggleExpanded,
  selectedTeamId,
  onSelect,
  onEdit,
  onDelete,
  matchingIds,
}: {
  team: Team;
  depth: number;
  expandedIds: Set<string>;
  toggleExpanded: (id: string) => void;
  selectedTeamId: string | null;
  onSelect: (teamId: string) => void;
  onEdit?: (team: Team) => void;
  onDelete?: (team: Team) => void;
  matchingIds: Set<string> | null;
}): React.ReactElement | null => {
  // If filtering is active and this node isn't in the matching set, hide it
  if (matchingIds && !matchingIds.has(team.id)) return null;

  const hasChildren = team.children.length > 0;
  const isSelected = selectedTeamId === team.id;
  const isExpanded = expandedIds.has(team.id);
  const hasActions = !!onEdit || !!onDelete;

  return (
    <>
      <button
        type="button"
        className={`group flex w-full items-center gap-2 border-b px-3 py-2 text-left text-sm transition-colors hover:bg-muted/50 ${
          isSelected ? "bg-muted/50" : ""
        }`}
        style={{ paddingLeft: `${depth * 1.25 + 0.75}rem` }}
        onClick={() => onSelect(team.id)}
      >
        {hasChildren ? (
          <button
            type="button"
            className="shrink-0 rounded p-0.5 hover:bg-muted"
            onClick={(e) => {
              e.stopPropagation();
              toggleExpanded(team.id);
            }}
          >
            {isExpanded ? (
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

        <span className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
          <Users className="size-3" />
          {team.totalMemberCount > 0 ? team.totalMemberCount : team.memberCount}
        </span>

        {hasActions && (
          <span
            className="flex shrink-0 items-center opacity-0 transition-opacity group-hover:opacity-100"
            onClick={(e) => e.stopPropagation()}
          >
            <DropdownMenu>
              <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" />}>
                <Ellipsis className="size-3.5" />
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end">
                {onEdit && (
                  <DropdownMenuItem onClick={() => onEdit(team)}>
                    <Pencil className="size-3.5" />
                    Edit
                  </DropdownMenuItem>
                )}
                {onDelete && (
                  <DropdownMenuItem className="text-destructive" onClick={() => onDelete(team)}>
                    <Trash2 className="size-3.5" />
                    Delete
                  </DropdownMenuItem>
                )}
              </DropdownMenuContent>
            </DropdownMenu>
          </span>
        )}
      </button>

      {isExpanded &&
        hasChildren &&
        team.children.map((child) => (
          <TeamTreeNode
            key={child.id}
            team={child}
            depth={depth + 1}
            expandedIds={expandedIds}
            toggleExpanded={toggleExpanded}
            selectedTeamId={selectedTeamId}
            onSelect={onSelect}
            onEdit={onEdit}
            onDelete={onDelete}
            matchingIds={matchingIds}
          />
        ))}
    </>
  );
};

export const TeamTree = ({
  roots,
  selectedTeamId,
  onSelect,
  onEdit,
  onDelete,
}: {
  roots: Team[];
  selectedTeamId: string | null;
  onSelect: (teamId: string) => void;
  onEdit?: (team: Team) => void;
  onDelete?: (team: Team) => void;
}): React.ReactElement => {
  const [filter, setFilter] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);

  // Compute initial expanded set (depth < 2) once
  const defaultExpanded = useMemo(() => {
    const ids = new Set<string>();
    const walk = (nodes: Team[], depth: number): void => {
      for (const t of nodes) {
        if (depth < 2) ids.add(t.id);
        walk(t.children, depth + 1);
      }
    };
    walk(roots, 0);
    return ids;
  }, [roots]);

  const [expandedIds, setExpandedIds] = useState<Set<string>>(defaultExpanded);

  const allIds = useMemo(() => collectIds(roots), [roots]);

  const toggleExpanded = useCallback((id: string) => {
    setExpandedIds((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }, []);

  const expandAll = useCallback(() => setExpandedIds(allIds), [allIds]);
  const collapseAll = useCallback(() => setExpandedIds(new Set()), []);

  // Filter matching: null means no filter active
  const matchingIds = useMemo(
    () => (filter.trim() ? findMatchingAncestors(roots, filter.trim()) : null),
    [roots, filter],
  );

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
      {/* Sticky toolbar: search + expand/collapse */}
      <div className="sticky top-0 z-10 flex items-center gap-2 border-b bg-background px-3 py-2">
        <div className="relative flex-1">
          <Search className="absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            ref={inputRef}
            placeholder="Filter teams..."
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
            className="h-8 pl-8 text-sm"
          />
        </div>
        <Button variant="ghost" size="icon-sm" title="Expand all" onClick={expandAll}>
          <ChevronsUpDown className="size-3.5" />
        </Button>
        <Button variant="ghost" size="icon-sm" title="Collapse all" onClick={collapseAll}>
          <ChevronsDownUp className="size-3.5" />
        </Button>
      </div>

      {roots.map((root) => (
        <TeamTreeNode
          key={root.id}
          team={root}
          depth={0}
          expandedIds={expandedIds}
          toggleExpanded={toggleExpanded}
          selectedTeamId={selectedTeamId}
          onSelect={onSelect}
          onEdit={onEdit}
          onDelete={onDelete}
          matchingIds={matchingIds}
        />
      ))}
    </div>
  );
};
