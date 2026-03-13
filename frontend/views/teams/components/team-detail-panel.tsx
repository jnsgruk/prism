import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardAction, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { X } from "lucide-react";

import { useGetTeam } from "@/views/teams/hooks/use-teams";

export const TeamDetailPanel = ({
  teamId,
  onClose,
}: {
  teamId: string;
  onClose: () => void;
}): React.ReactElement => {
  const { data, isLoading, error } = useGetTeam(teamId);

  if (isLoading) {
    return (
      <Card>
        <CardContent className="p-6">
          <p className="text-sm text-muted-foreground">Loading team details...</p>
        </CardContent>
      </Card>
    );
  }

  if (error || !data?.team) {
    return (
      <Card>
        <CardContent className="p-6">
          <Alert variant="destructive">Failed to load team details.</Alert>
        </CardContent>
      </Card>
    );
  }

  const { team, members } = data;

  return (
    <Card className="overflow-hidden">
      <CardHeader>
        <CardTitle className="truncate">{team.name}</CardTitle>
        <p className="truncate text-sm text-muted-foreground">{team.orgName}</p>
        <CardAction>
          <Button variant="ghost" size="icon" onClick={onClose}>
            <X className="size-4" />
            <span className="sr-only">Close</span>
          </Button>
        </CardAction>
      </CardHeader>
      <CardContent>
        <h3 className="mb-3 text-sm font-medium">Members ({members.length})</h3>
        {members.length === 0 ? (
          <p className="text-sm text-muted-foreground">No members in this team.</p>
        ) : (
          <div className="space-y-2">
            {members.map((person) => (
              <div
                key={person.id}
                className="flex flex-wrap items-center justify-between gap-2 rounded border px-4 py-3"
              >
                <div className="min-w-0">
                  <p className="truncate text-sm font-medium">{person.name}</p>
                  {person.email && (
                    <p className="truncate text-xs text-muted-foreground">{person.email}</p>
                  )}
                </div>
                <div className="flex flex-wrap gap-1">
                  {person.identities.map((id) => (
                    <Badge key={`${id.platform}-${id.username}`} variant="secondary">
                      {id.platform}
                    </Badge>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
};
