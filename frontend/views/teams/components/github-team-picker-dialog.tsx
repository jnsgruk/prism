import { useState } from "react";
import type { GitHubTeam } from "@ps/api/gen/prism/v1/org_pb";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { GitBranch, Plus } from "lucide-react";

import { useAssignGithubTeam } from "@/views/admin/hooks/use-admin";
import { useListGithubTeams } from "@/views/teams/hooks/use-teams";

export const GithubTeamPickerDialog = ({
  teamId,
  open,
  onOpenChange,
  alreadyAssigned,
}: {
  teamId: string;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  alreadyAssigned: string[];
}): React.ReactElement => {
  const [search, setSearch] = useState("");
  const { data: allTeams, isLoading } = useListGithubTeams(search || undefined);
  const assign = useAssignGithubTeam();

  const available = allTeams?.filter((t) => !alreadyAssigned.includes(t.id)) ?? [];

  const handleAssign = (githubTeamId: string): void => {
    assign.mutate({ teamId, githubTeamId }, { onSuccess: () => onOpenChange(false) });
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-h-[80vh] sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Link GitHub Team</DialogTitle>
          <DialogDescription>
            Search and select a GitHub team to link to this Prism team. Linked teams scope which
            repositories are ingested.
          </DialogDescription>
        </DialogHeader>

        <Input
          placeholder="Search GitHub teams..."
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />

        <TeamList isLoading={isLoading} teams={available} search={search} onAssign={handleAssign} />
      </DialogContent>
    </Dialog>
  );
};

const TeamList = ({
  isLoading,
  teams,
  search,
  onAssign,
}: {
  isLoading: boolean;
  teams: GitHubTeam[];
  search: string;
  onAssign: (id: string) => void;
}): React.ReactElement => {
  if (isLoading) {
    return <p className="py-4 text-center text-sm text-muted-foreground">Loading teams...</p>;
  }

  if (teams.length === 0) {
    return (
      <p className="py-4 text-center text-sm text-muted-foreground">
        {search
          ? "No matching teams found."
          : "No GitHub teams discovered yet. Run a team sync first."}
      </p>
    );
  }

  return (
    <div className="max-h-80 space-y-1 overflow-y-auto">
      {teams.map((gt) => (
        <button
          key={gt.id}
          type="button"
          className="flex w-full items-center justify-between gap-2 rounded-md px-3 py-2.5 text-left hover:bg-accent"
          onClick={() => onAssign(gt.id)}
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
            <Plus className="size-4 text-muted-foreground" />
          </div>
        </button>
      ))}
    </div>
  );
};
