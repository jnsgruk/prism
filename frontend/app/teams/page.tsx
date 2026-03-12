"use client";

import { Users, ChevronRight, Upload, AlertCircle } from "lucide-react";
import { useState } from "react";

import { useListTeams, useGetTeam, useImportDirectory } from "@ps/hooks";
import { cn } from "@ps/utils/cn";

const TeamDetailPanel = ({ teamId, onClose }: { teamId: string; onClose: () => void }) => {
  const { data, isLoading, error } = useGetTeam(teamId);

  if (isLoading) {
    return (
      <div className="rounded-lg border p-6">
        <p className="text-sm text-muted-foreground">Loading team details...</p>
      </div>
    );
  }

  if (error || !data?.team) {
    return (
      <div className="rounded-lg border p-6">
        <p className="text-sm text-red-600">Failed to load team details.</p>
      </div>
    );
  }

  const { team, members } = data;

  return (
    <div className="rounded-lg border">
      <div className="flex items-center justify-between border-b px-6 py-4">
        <div>
          <h2 className="text-lg font-semibold">{team.name}</h2>
          <p className="text-sm text-muted-foreground">{team.orgName}</p>
        </div>
        <button onClick={onClose} className="text-sm text-muted-foreground hover:text-foreground">
          Close
        </button>
      </div>

      <div className="p-6">
        <h3 className="mb-3 text-sm font-medium">Members ({members.length})</h3>
        {members.length === 0 ? (
          <p className="text-sm text-muted-foreground">No members in this team.</p>
        ) : (
          <div className="space-y-2">
            {members.map((person) => (
              <div key={person.id} className="flex items-center justify-between rounded border px-4 py-3">
                <div>
                  <p className="text-sm font-medium">{person.name}</p>
                  {person.email && <p className="text-xs text-muted-foreground">{person.email}</p>}
                </div>
                <div className="flex gap-1">
                  {person.identities.map((id) => (
                    <span key={`${id.platform}-${id.username}`} className="rounded-full bg-muted px-2 py-0.5 text-xs">
                      {id.platform}
                    </span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};

const ImportDirectoryDialog = ({ onClose }: { onClose: () => void }) => {
  const importDirectory = useImportDirectory();
  const [dragActive, setDragActive] = useState(false);

  const handleFile = async (file: File) => {
    const buffer = await file.arrayBuffer();
    importDirectory.mutate(new Uint8Array(buffer));
  };

  const handleDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragActive(false);
    const file = e.dataTransfer.files[0];
    if (file) handleFile(file);
  };

  const handleFileInput = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (file) handleFile(file);
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/50">
      <div className="w-full max-w-lg rounded-lg border bg-background p-6 shadow-lg">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="text-lg font-semibold">Import Directory</h2>
          <button onClick={onClose} className="text-sm text-muted-foreground hover:text-foreground">
            Cancel
          </button>
        </div>

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
          <Upload className="mb-2 h-8 w-8 text-muted-foreground" />
          <p className="mb-1 text-sm font-medium">Drop a CSV or JSON file here</p>
          <p className="mb-3 text-xs text-muted-foreground">or click to browse</p>
          <label className="cursor-pointer rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground">
            Browse Files
            <input type="file" accept=".csv,.json" onChange={handleFileInput} className="hidden" />
          </label>
        </div>

        {importDirectory.isPending && <p className="mt-4 text-sm text-muted-foreground">Importing...</p>}

        {importDirectory.isSuccess && (
          <div className="mt-4 rounded border border-green-200 bg-green-50 p-4 dark:border-green-900 dark:bg-green-950">
            <p className="text-sm font-medium text-green-800 dark:text-green-200">Import complete</p>
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
          <div className="mt-4 flex items-start gap-2 rounded border border-red-200 bg-red-50 p-4 dark:border-red-900 dark:bg-red-950">
            <AlertCircle className="mt-0.5 h-4 w-4 text-red-600" />
            <p className="text-sm text-red-700 dark:text-red-300">
              {importDirectory.error instanceof Error ? importDirectory.error.message : "Import failed"}
            </p>
          </div>
        )}
      </div>
    </div>
  );
};

const TeamsPage = () => {
  const [selectedTeamId, setSelectedTeamId] = useState<string | null>(null);
  const [showImport, setShowImport] = useState(false);
  const { data: teams, isLoading, error } = useListTeams();

  return (
    <div className="p-8">
      <div className="mb-6 flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">Teams</h1>
          <p className="mt-1 text-sm text-muted-foreground">Manage your organization structure and team memberships.</p>
        </div>
        <button
          onClick={() => setShowImport(true)}
          className="flex items-center gap-2 rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground"
        >
          <Upload className="h-4 w-4" />
          Import Directory
        </button>
      </div>

      {isLoading && <p className="text-sm text-muted-foreground">Loading teams...</p>}

      {error && (
        <div className="flex items-start gap-2 rounded border border-red-200 bg-red-50 p-4 dark:border-red-900 dark:bg-red-950">
          <AlertCircle className="mt-0.5 h-4 w-4 text-red-600" />
          <p className="text-sm text-red-700 dark:text-red-300">Failed to load teams.</p>
        </div>
      )}

      {teams && teams.length === 0 && (
        <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
          <Users className="mb-3 h-10 w-10 text-muted-foreground" />
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
                  <span className="rounded-full bg-muted px-2 py-0.5 text-xs">
                    {team.memberCount} {team.memberCount === 1 ? "member" : "members"}
                  </span>
                  <ChevronRight className="h-4 w-4 text-muted-foreground" />
                </div>
              </button>
            ))}
          </div>

          <div>
            {selectedTeamId ? (
              <TeamDetailPanel teamId={selectedTeamId} onClose={() => setSelectedTeamId(null)} />
            ) : (
              <div className="flex h-full items-center justify-center rounded-lg border-2 border-dashed p-12">
                <p className="text-sm text-muted-foreground">Select a team to view its members.</p>
              </div>
            )}
          </div>
        </div>
      )}

      {showImport && <ImportDirectoryDialog onClose={() => setShowImport(false)} />}
    </div>
  );
};

export default TeamsPage;
