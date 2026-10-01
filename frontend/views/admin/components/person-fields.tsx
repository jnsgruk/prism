import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { PersonDraft } from "@/views/admin/lib/person-form";

import type { Team } from "@ps/api/gen/canonical/prism/v1/org_pb";

export const PersonFields = ({
  draft,
  onChange,
  teams,
}: {
  draft: PersonDraft;
  onChange: (draft: PersonDraft) => void;
  teams: Team[];
}): React.ReactElement => (
  <>
    <div className="space-y-2">
      <Label htmlFor="person-name">Name</Label>
      <Input
        id="person-name"
        value={draft.name}
        onChange={(e) => onChange({ ...draft, name: e.target.value })}
        required
      />
    </div>

    <div className="space-y-2">
      <Label htmlFor="person-email">Email (optional)</Label>
      <Input
        id="person-email"
        type="email"
        value={draft.email}
        onChange={(e) => onChange({ ...draft, email: e.target.value })}
      />
    </div>

    <div className="space-y-2">
      <Label htmlFor="person-level">Level / Title (optional)</Label>
      <Input id="person-level" value={draft.level} onChange={(e) => onChange({ ...draft, level: e.target.value })} />
    </div>

    <div className="space-y-2">
      <Label htmlFor="person-team">Team</Label>
      <Select value={draft.teamId} onValueChange={(value) => value !== null && onChange({ ...draft, teamId: value })}>
        <SelectTrigger id="person-team" className="w-full min-w-0">
          <SelectValue>{teams.find((team) => team.id === draft.teamId)?.name ?? "No team"}</SelectValue>
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="">No team</SelectItem>
          {draft.teamId && !teams.some((team) => team.id === draft.teamId) && (
            <SelectItem value={draft.teamId}>Current team</SelectItem>
          )}
          {teams.map((team) => (
            <SelectItem key={team.id} value={team.id}>
              {team.name}
            </SelectItem>
          ))}
        </SelectContent>
      </Select>
    </div>
  </>
);
