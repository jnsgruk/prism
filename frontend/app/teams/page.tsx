"use client";

import { PageHeader } from "@/components/page-header";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { AlertCircle, ChevronRight, Upload, Users } from "lucide-react";
import { useState } from "react";

import { useListTeams, useGetTeam, useImportDirectory } from "@ps/hooks";
import { cn } from "@ps/utils/cn";

const TeamDetailPanel = ({
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
    <Card>
      <CardHeader className="flex-row items-center justify-between">
        <div>
          <CardTitle>{team.name}</CardTitle>
          <p className="text-sm text-muted-foreground">{team.orgName}</p>
        </div>
        <Button variant="ghost" size="sm" onClick={onClose}>
          Close
        </Button>
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
                className="flex items-center justify-between rounded border px-4 py-3"
              >
                <div>
                  <p className="text-sm font-medium">{person.name}</p>
                  {person.email && <p className="text-xs text-muted-foreground">{person.email}</p>}
                </div>
                <div className="flex gap-1">
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

const ImportDirectoryDialog = (): React.ReactElement => {
  const importDirectory = useImportDirectory();
  const [dragActive, setDragActive] = useState(false);
  const [open, setOpen] = useState(false);

  const handleFile = async (file: File): Promise<void> => {
    const buffer = await file.arrayBuffer();
    importDirectory.mutate(new Uint8Array(buffer));
  };

  const handleDrop = (e: React.DragEvent): void => {
    e.preventDefault();
    setDragActive(false);
    const file = e.dataTransfer.files[0];
    if (file) handleFile(file);
  };

  const handleFileInput = (e: React.ChangeEvent<HTMLInputElement>): void => {
    const file = e.target.files?.[0];
    if (file) handleFile(file);
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button />}>
        <Upload className="size-4" />
        Import Directory
      </DialogTrigger>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Import Directory</DialogTitle>
          <DialogDescription>
            Upload an HTML or JSON directory export to populate teams and people.
          </DialogDescription>
        </DialogHeader>

        <div
          onDragOver={(e) => {
            e.preventDefault();
            setDragActive(true);
          }}
          onDragLeave={() => setDragActive(false)}
          onDrop={handleDrop}
          className={cn(
            "flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-8",
            dragActive ? "border-primary bg-primary/5" : "border-muted-foreground/25",
          )}
        >
          <Upload className="mb-2 size-8 text-muted-foreground" />
          <p className="mb-1 text-sm font-medium">Drop an HTML or JSON file here</p>
          <p className="mb-3 text-xs text-muted-foreground">or click to browse</p>
          <Button render={<label className="cursor-pointer" />}>
            Browse Files
            <input type="file" accept=".html,.json" onChange={handleFileInput} className="hidden" />
          </Button>
        </div>

        {importDirectory.isPending && <p className="text-sm text-muted-foreground">Importing...</p>}

        {importDirectory.isSuccess && (
          <div className="rounded border border-green-200 bg-green-50 p-4 dark:border-green-900 dark:bg-green-950">
            <p className="text-sm font-medium text-green-800 dark:text-green-200">
              Import complete
            </p>
            <ul className="mt-1 text-xs text-green-700 dark:text-green-300">
              <li>{importDirectory.data.peopleImported} people imported</li>
              <li>{importDirectory.data.teamsCreated} teams created</li>
              <li>{importDirectory.data.identitiesMapped} identities mapped</li>
            </ul>
            {importDirectory.data.warnings.length > 0 && (
              <div className="mt-2">
                <p className="text-xs font-medium text-amber-700 dark:text-amber-300">Warnings:</p>
                {importDirectory.data.warnings.map((w, i) => (
                  <p key={i} className="text-xs text-amber-600 dark:text-amber-400">
                    {w}
                  </p>
                ))}
              </div>
            )}
          </div>
        )}

        {importDirectory.isError && (
          <Alert variant="destructive">
            <AlertCircle className="size-4" />
            {importDirectory.error instanceof Error
              ? importDirectory.error.message
              : "Import failed"}
          </Alert>
        )}
      </DialogContent>
    </Dialog>
  );
};

const TeamsPage = (): React.ReactElement => {
  const [selectedTeamId, setSelectedTeamId] = useState<string | null>(null);
  const { data: teams, isLoading, error } = useListTeams();

  return (
    <>
      <PageHeader
        title="Teams"
        description="Manage your organization structure and team memberships"
        actions={<ImportDirectoryDialog />}
      />
      <div className="flex-1 p-6">
        {isLoading && <p className="text-sm text-muted-foreground">Loading teams...</p>}

        {error && (
          <Alert variant="destructive">
            <AlertCircle className="size-4" />
            Failed to load teams.
          </Alert>
        )}

        {teams && teams.length === 0 && (
          <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
            <Users className="mb-3 size-10 text-muted-foreground" />
            <p className="mb-1 font-medium">No teams yet</p>
            <p className="text-sm text-muted-foreground">Import a directory file to get started.</p>
          </div>
        )}

        {teams && teams.length > 0 && (
          <div className="grid gap-6 lg:grid-cols-2">
            <div className="space-y-2">
              {teams.map((team) => (
                <button
                  key={team.id}
                  onClick={() => setSelectedTeamId(team.id)}
                  className={cn(
                    "flex w-full items-center justify-between rounded-lg border px-4 py-3 text-left transition-colors hover:bg-muted/50",
                    selectedTeamId === team.id && "border-primary bg-muted/50",
                  )}
                >
                  <div>
                    <p className="text-sm font-medium">{team.name}</p>
                    <p className="text-xs text-muted-foreground">{team.orgName}</p>
                  </div>
                  <div className="flex items-center gap-2">
                    <Badge variant="secondary">
                      {team.memberCount} {team.memberCount === 1 ? "member" : "members"}
                    </Badge>
                    <ChevronRight className="size-4 text-muted-foreground" />
                  </div>
                </button>
              ))}
            </div>

            <div>
              {selectedTeamId ? (
                <TeamDetailPanel teamId={selectedTeamId} onClose={() => setSelectedTeamId(null)} />
              ) : (
                <div className="flex h-full items-center justify-center rounded-lg border-2 border-dashed p-12">
                  <p className="text-sm text-muted-foreground">
                    Select a team to view its members.
                  </p>
                </div>
              )}
            </div>
          </div>
        )}
      </div>
    </>
  );
};

export default TeamsPage;
