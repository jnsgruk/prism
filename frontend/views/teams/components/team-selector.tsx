import { Badge } from "@/components/ui/badge";
import {
  Command,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { ChevronsUpDown, Users } from "lucide-react";
import { useMemo, useState } from "react";

import type { Team } from "@ps/api/gen/prism/v1/org_pb";

import { flattenTree, teamTypeBadgeVariant, teamTypeLabel } from "@/views/teams/hooks/use-teams";

export const TeamSelector = ({
  roots,
  selectedTeam,
  onSelect,
}: {
  roots: Team[];
  selectedTeam: Team | undefined;
  onSelect: (teamId: string) => void;
}): React.ReactElement => {
  const [open, setOpen] = useState(false);
  const [search, setSearch] = useState("");

  const flat = useMemo(() => flattenTree(roots), [roots]);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <button
            type="button"
            className="inline-flex items-center gap-1.5 rounded-md px-1.5 py-0.5 text-sm font-medium hover:bg-muted"
          />
        }
      >
        {selectedTeam?.name ?? "Select team..."}
        <ChevronsUpDown className="size-3 shrink-0 text-muted-foreground" />
      </PopoverTrigger>
      <PopoverContent className="w-80 p-0" align="start">
        <Command shouldFilter={false}>
          <CommandInput placeholder="Search teams..." value={search} onValueChange={setSearch} />
          <CommandList>
            <CommandEmpty>No teams found.</CommandEmpty>
            {flat
              .filter(
                ({ team }) => !search || team.name.toLowerCase().includes(search.toLowerCase()),
              )
              .map(({ team, depth }) => (
                <CommandItem
                  key={team.id}
                  value={team.id}
                  data-checked={selectedTeam?.id === team.id ? "true" : undefined}
                  onSelect={() => {
                    onSelect(team.id);
                    setOpen(false);
                    setSearch("");
                  }}
                >
                  <span
                    className="flex items-center gap-2 truncate"
                    style={{ paddingLeft: `${depth * 0.75}rem` }}
                  >
                    <span className="truncate">{team.name}</span>
                    <Badge
                      variant={teamTypeBadgeVariant(team.teamType)}
                      className="shrink-0 text-[10px]"
                    >
                      {teamTypeLabel(team.teamType)}
                    </Badge>
                    <span className="flex shrink-0 items-center gap-1 text-xs text-muted-foreground">
                      <Users className="size-3" />
                      {team.totalMemberCount > 0 ? team.totalMemberCount : team.memberCount}
                    </span>
                  </span>
                </CommandItem>
              ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
};
