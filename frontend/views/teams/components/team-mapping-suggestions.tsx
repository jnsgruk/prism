import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Check, ChevronDown, ChevronRight, GitBranch, Lightbulb, X } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { toast } from "sonner";

import type { TeamMappingSuggestion } from "@ps/api/gen/prism/v1/org_pb";

import {
  useAssignGithubTeam,
  useDismissTeamMappingSuggestion,
} from "@/views/admin/hooks/use-admin";
import { useGetTeamMappingSuggestions } from "@/views/teams/hooks/use-teams";

const STORAGE_KEY = "prism:suggestions-collapsed";

const getCollapsedTeams = (): Set<string> => {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return new Set(JSON.parse(raw) as string[]);
  } catch {
    // ignore
  }
  return new Set();
};

const setCollapsedTeams = (ids: Set<string>): void => {
  localStorage.setItem(STORAGE_KEY, JSON.stringify([...ids]));
};

const formatPct = (v: number): string => `${Math.round(v * 100)}%`;

const SuggestionRow = ({
  suggestion,
  teamId,
}: {
  suggestion: TeamMappingSuggestion;
  teamId: string;
}): React.ReactElement => {
  const assign = useAssignGithubTeam();
  const dismiss = useDismissTeamMappingSuggestion();
  const isPending = assign.isPending || dismiss.isPending;

  const handleApply = (): void => {
    assign.mutate(
      { teamId, githubTeamId: suggestion.githubTeamId },
      {
        onSuccess: () => toast.success(`Linked ${suggestion.githubTeamName}`),
        onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to link"),
      },
    );
  };

  const handleDismiss = (): void => {
    dismiss.mutate(
      { teamId, githubTeamId: suggestion.githubTeamId },
      {
        onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to dismiss"),
      },
    );
  };

  return (
    <div className="flex items-center justify-between gap-2 rounded border px-3 py-2">
      <div className="flex min-w-0 items-center gap-2">
        <GitBranch className="size-4 shrink-0 text-muted-foreground" />
        <div className="min-w-0">
          <p className="truncate text-sm font-medium">{suggestion.githubTeamName}</p>
          <p className="truncate text-xs text-muted-foreground">
            {suggestion.githubOrg}/{suggestion.githubTeamSlug}
          </p>
        </div>
      </div>
      <div className="flex items-center gap-2">
        <Badge variant="outline" className="text-xs">
          {Number(suggestion.overlapCount)} shared
        </Badge>
        <Badge variant="outline" className="text-xs">
          {formatPct(suggestion.githubCoverage)} GH
        </Badge>
        <Badge variant="outline" className="text-xs">
          {formatPct(suggestion.prismCoverage)} team
        </Badge>
        <Button
          variant="ghost"
          size="icon"
          className="size-7"
          disabled={isPending}
          onClick={handleApply}
          title="Apply suggestion"
        >
          <Check className="size-3.5 text-green-600" />
        </Button>
        <Button
          variant="ghost"
          size="icon"
          className="size-7"
          disabled={isPending}
          onClick={handleDismiss}
          title="Dismiss suggestion"
        >
          <X className="size-3.5" />
        </Button>
      </div>
    </div>
  );
};

export const TeamMappingSuggestions = ({
  teamId,
}: {
  teamId: string;
}): React.ReactElement | null => {
  const { data: allSuggestions } = useGetTeamMappingSuggestions();
  const [collapsedIds, setCollapsedIds] = useState(getCollapsedTeams);

  const isCollapsed = collapsedIds.has(teamId);

  const toggleCollapsed = useCallback(() => {
    setCollapsedIds((prev) => {
      const next = new Set(prev);
      if (next.has(teamId)) next.delete(teamId);
      else next.add(teamId);
      setCollapsedTeams(next);
      return next;
    });
  }, [teamId]);

  const suggestions = useMemo(
    () =>
      (allSuggestions?.filter((s) => s.prismTeamId === teamId) ?? []).toSorted(
        (a, b) => b.prismCoverage - a.prismCoverage,
      ),
    [allSuggestions, teamId],
  );

  if (suggestions.length === 0) return null;

  return (
    <Card className="border-amber-200 bg-amber-50/50 dark:border-amber-800 dark:bg-amber-950/30">
      <CardHeader className="pb-2">
        <CardTitle
          className="flex cursor-pointer items-center gap-2 text-sm"
          onClick={toggleCollapsed}
        >
          <Lightbulb className="size-4 text-amber-600" />
          Suggested Mappings ({suggestions.length})
        </CardTitle>
        <CardAction>
          <Button variant="ghost" size="icon-sm" onClick={toggleCollapsed}>
            {isCollapsed ? <ChevronRight className="size-4" /> : <ChevronDown className="size-4" />}
          </Button>
        </CardAction>
      </CardHeader>
      {!isCollapsed && (
        <CardContent>
          <p className="mb-3 text-xs text-muted-foreground">
            Based on member overlap between GitHub teams and this Prism team.
          </p>
          <div className="space-y-2">
            {suggestions.map((s) => (
              <SuggestionRow key={s.githubTeamId} suggestion={s} teamId={teamId} />
            ))}
          </div>
        </CardContent>
      )}
    </Card>
  );
};
