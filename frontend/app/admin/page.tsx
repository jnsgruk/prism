"use client";

import { Plus, Plug, Key, AlertCircle, CheckCircle2, Loader2, Trash2, Settings2 } from "lucide-react";
import { useState } from "react";

import type { SourceConfig } from "@ps/api/gen/prism/v1/config_pb";
import {
  useListSources,
  useCreateSource,
  useUpdateSource,
  useDeleteSource,
  useSetSecret,
  useTestConnection,
} from "@ps/hooks";
import { cn } from "@ps/utils/cn";

const SOURCE_TYPES = [
  { value: "github", label: "GitHub" },
  { value: "jira", label: "Jira" },
  { value: "discourse", label: "Discourse" },
  { value: "launchpad", label: "Launchpad" },
  { value: "google_drive", label: "Google Drive" },
  { value: "mailing_list", label: "Mailing List" },
];

const SECRET_KEYS_BY_TYPE: Record<string, string[]> = {
  github: ["api_token"],
  jira: ["api_token", "email"],
  discourse: ["api_key"],
  launchpad: ["oauth_token"],
  google_drive: ["service_account_key"],
  mailing_list: [],
};

const CreateSourceDialog = ({ onClose }: { onClose: () => void }) => {
  const createSource = useCreateSource();
  const [name, setName] = useState("");
  const [sourceType, setSourceType] = useState("github");

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    createSource.mutate({ sourceType, name }, { onSuccess: () => onClose() });
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0" onClick={onClose} />
      <div className="relative w-full max-w-md rounded-lg border bg-background p-6 shadow-xl">
        <h2 className="mb-4 text-lg font-semibold">Add Source</h2>

        <form onSubmit={handleSubmit} className="space-y-4">
          <div>
            <label htmlFor="source-name" className="mb-1 block text-sm font-medium">
              Name
            </label>
            <input
              id="source-name"
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              placeholder="e.g. canonical/ubuntu"
              className="block w-full rounded border px-3 py-2 text-sm"
              required
            />
          </div>

          <div>
            <label htmlFor="source-type" className="mb-1 block text-sm font-medium">
              Type
            </label>
            <select
              id="source-type"
              value={sourceType}
              onChange={(e) => setSourceType(e.target.value)}
              className="block w-full rounded border bg-background px-3 py-2 text-sm"
            >
              {SOURCE_TYPES.map((t) => (
                <option key={t.value} value={t.value}>
                  {t.label}
                </option>
              ))}
            </select>
          </div>

          {createSource.isError && (
            <p className="text-sm text-red-600">
              {createSource.error instanceof Error ? createSource.error.message : "Failed to create source"}
            </p>
          )}

          <div className="flex justify-end gap-2">
            <button type="button" onClick={onClose} className="rounded border px-4 py-2 text-sm">
              Cancel
            </button>
            <button
              type="submit"
              disabled={createSource.isPending || !name.trim()}
              className="rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:opacity-50"
            >
              {createSource.isPending ? "Creating..." : "Create"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};

const SetSecretDialog = ({ source, onClose }: { source: SourceConfig; onClose: () => void }) => {
  const setSecret = useSetSecret();
  const secretKeys = SECRET_KEYS_BY_TYPE[source.sourceType] ?? ["api_token"];
  const [selectedKey, setSelectedKey] = useState(secretKeys[0] ?? "api_token");
  const [secretValue, setSecretValue] = useState("");

  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    setSecret.mutate({ sourceId: source.id, secretKey: selectedKey, secretValue }, { onSuccess: () => onClose() });
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <div className="absolute inset-0" onClick={onClose} />
      <div className="relative w-full max-w-md rounded-lg border bg-background p-6 shadow-xl">
        <h2 className="mb-1 text-lg font-semibold">Set Secret</h2>
        <p className="mb-4 text-sm text-muted-foreground">{source.name}</p>

        <form onSubmit={handleSubmit} className="space-y-4">
          {secretKeys.length > 1 && (
            <div>
              <label htmlFor="secret-key" className="mb-1 block text-sm font-medium">
                Secret Key
              </label>
              <select
                id="secret-key"
                value={selectedKey}
                onChange={(e) => setSelectedKey(e.target.value)}
                className="block w-full rounded border bg-background px-3 py-2 text-sm"
              >
                {secretKeys.map((k) => (
                  <option key={k} value={k}>
                    {k}
                  </option>
                ))}
              </select>
            </div>
          )}

          <div>
            <label htmlFor="secret-value" className="mb-1 block text-sm font-medium">
              {secretKeys.length <= 1 ? `Value (${selectedKey})` : "Value"}
            </label>
            <input
              id="secret-value"
              type="password"
              value={secretValue}
              onChange={(e) => setSecretValue(e.target.value)}
              placeholder="Paste your token here"
              className="block w-full rounded border px-3 py-2 font-mono text-sm"
              required
            />
          </div>

          {setSecret.isError && (
            <p className="text-sm text-red-600">
              {setSecret.error instanceof Error ? setSecret.error.message : "Failed to set secret"}
            </p>
          )}

          <div className="flex justify-end gap-2">
            <button type="button" onClick={onClose} className="rounded border px-4 py-2 text-sm">
              Cancel
            </button>
            <button
              type="submit"
              disabled={setSecret.isPending || !secretValue.trim()}
              className="rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:opacity-50"
            >
              {setSecret.isPending ? "Saving..." : "Save Secret"}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
};

const SourceRow = ({ source }: { source: SourceConfig }) => {
  const updateSource = useUpdateSource();
  const deleteSource = useDeleteSource();
  const testConnection = useTestConnection();
  const [showSecret, setShowSecret] = useState(false);

  const secretEntries = Object.entries(source.secretStatus);
  const allSecretsSet = secretEntries.length > 0 && secretEntries.every(([, set]) => set);
  const sourceLabel = SOURCE_TYPES.find((t) => t.value === source.sourceType)?.label ?? source.sourceType;

  const handleToggleEnabled = () => {
    updateSource.mutate({ sourceId: source.id, enabled: !source.enabled });
  };

  const handleDelete = () => {
    if (confirm(`Delete source "${source.name}"?`)) {
      deleteSource.mutate(source.id);
    }
  };

  const handleTestConnection = () => {
    testConnection.mutate(source.id);
  };

  return (
    <>
      <div className="flex items-center justify-between rounded-lg border px-4 py-3">
        <div className="flex items-center gap-3">
          <button
            onClick={handleToggleEnabled}
            disabled={updateSource.isPending}
            className={cn(
              "relative h-5 w-9 rounded-full transition-colors",
              source.enabled ? "bg-green-500" : "bg-muted",
            )}
            title={source.enabled ? "Disable source" : "Enable source"}
          >
            <span
              className={cn(
                "absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform",
                source.enabled ? "left-[18px]" : "left-0.5",
              )}
            />
          </button>

          <div>
            <p className="text-sm font-medium">{source.name}</p>
            <div className="flex items-center gap-2">
              <span className="rounded bg-muted px-1.5 py-0.5 text-xs">{sourceLabel}</span>
              {allSecretsSet ? (
                <span className="flex items-center gap-1 text-xs text-green-600">
                  <Key className="h-3 w-3" /> Configured
                </span>
              ) : (
                <span className="flex items-center gap-1 text-xs text-amber-600">
                  <Key className="h-3 w-3" /> Needs secret
                </span>
              )}
            </div>
          </div>
        </div>

        <div className="flex items-center gap-1">
          <button
            onClick={() => setShowSecret(true)}
            className="rounded p-2 text-muted-foreground hover:bg-muted hover:text-foreground"
            title="Set secret"
          >
            <Settings2 className="h-4 w-4" />
          </button>

          <button
            onClick={handleTestConnection}
            disabled={testConnection.isPending}
            className="rounded p-2 text-muted-foreground hover:bg-muted hover:text-foreground"
            title="Test connection"
          >
            {testConnection.isPending ? <Loader2 className="h-4 w-4 animate-spin" /> : <Plug className="h-4 w-4" />}
          </button>

          <button
            onClick={handleDelete}
            disabled={deleteSource.isPending}
            className="rounded p-2 text-muted-foreground hover:bg-muted hover:text-red-600"
            title="Delete source"
          >
            <Trash2 className="h-4 w-4" />
          </button>
        </div>
      </div>

      {testConnection.isSuccess && (
        <div
          className={cn(
            "mx-4 -mt-1 mb-1 rounded-b border border-t-0 px-4 py-2",
            testConnection.data.success
              ? "border-green-200 bg-green-50 dark:border-green-900 dark:bg-green-950"
              : "border-red-200 bg-red-50 dark:border-red-900 dark:bg-red-950",
          )}
        >
          {testConnection.data.success ? (
            <p className="flex items-center gap-1 text-sm text-green-700 dark:text-green-300">
              <CheckCircle2 className="h-4 w-4" /> Connection successful
            </p>
          ) : (
            <p className="flex items-center gap-1 text-sm text-red-700 dark:text-red-300">
              <AlertCircle className="h-4 w-4" /> {testConnection.data.errorMessage || "Connection failed"}
            </p>
          )}
        </div>
      )}

      {testConnection.isError && (
        <div className="mx-4 -mt-1 mb-1 rounded-b border border-t-0 border-red-200 bg-red-50 px-4 py-2 dark:border-red-900 dark:bg-red-950">
          <p className="flex items-center gap-1 text-sm text-red-700 dark:text-red-300">
            <AlertCircle className="h-4 w-4" />{" "}
            {testConnection.error instanceof Error ? testConnection.error.message : "Test failed"}
          </p>
        </div>
      )}

      {showSecret && <SetSecretDialog source={source} onClose={() => setShowSecret(false)} />}
    </>
  );
};

const SourcesTab = () => {
  const { data: sources, isLoading, error } = useListSources();
  const [showCreate, setShowCreate] = useState(false);

  return (
    <div>
      <div className="mb-4 flex items-center justify-between">
        <p className="text-sm text-muted-foreground">Configure data sources and their API credentials.</p>
        <button
          onClick={() => setShowCreate(true)}
          className="flex items-center gap-2 rounded bg-primary px-4 py-2 text-sm font-medium text-primary-foreground"
        >
          <Plus className="h-4 w-4" />
          Add Source
        </button>
      </div>

      {isLoading && <p className="text-sm text-muted-foreground">Loading sources...</p>}

      {error && (
        <div className="flex items-start gap-2 rounded border border-red-200 bg-red-50 p-4 dark:border-red-900 dark:bg-red-950">
          <AlertCircle className="mt-0.5 h-4 w-4 text-red-600" />
          <p className="text-sm text-red-700 dark:text-red-300">Failed to load sources.</p>
        </div>
      )}

      {sources && sources.length === 0 && (
        <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
          <Plug className="mb-3 h-10 w-10 text-muted-foreground" />
          <p className="mb-1 font-medium">No sources configured</p>
          <p className="text-sm text-muted-foreground">Add a source to start ingesting data.</p>
        </div>
      )}

      {sources && sources.length > 0 && (
        <div className="space-y-2">
          {sources.map((source) => (
            <SourceRow key={source.id} source={source} />
          ))}
        </div>
      )}

      {showCreate && <CreateSourceDialog onClose={() => setShowCreate(false)} />}
    </div>
  );
};

const ApiTokensTab = () => (
  <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
    <Key className="mb-3 h-10 w-10 text-muted-foreground" />
    <p className="mb-1 font-medium">API Tokens</p>
    <p className="text-sm text-muted-foreground">API token management will be implemented in a future workstream.</p>
  </div>
);

const AdminPage = () => {
  const [activeTab, setActiveTab] = useState<"sources" | "tokens">("sources");

  return (
    <div className="p-8">
      <h1 className="mb-6 text-2xl font-bold">Admin</h1>

      <div className="mb-6 flex gap-1 rounded-lg border p-1">
        <button
          onClick={() => setActiveTab("sources")}
          className={cn(
            "flex items-center gap-2 rounded-md px-4 py-2 text-sm font-medium transition-colors",
            activeTab === "sources" ? "bg-muted text-foreground" : "text-muted-foreground hover:text-foreground",
          )}
        >
          <Plug className="h-4 w-4" />
          Sources
        </button>
        <button
          onClick={() => setActiveTab("tokens")}
          className={cn(
            "flex items-center gap-2 rounded-md px-4 py-2 text-sm font-medium transition-colors",
            activeTab === "tokens" ? "bg-muted text-foreground" : "text-muted-foreground hover:text-foreground",
          )}
        >
          <Key className="h-4 w-4" />
          API Tokens
        </button>
      </div>

      {activeTab === "sources" ? <SourcesTab /> : <ApiTokensTab />}
    </div>
  );
};

export default AdminPage;
