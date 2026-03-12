"use client";

import { PageHeader } from "@/components/page-header";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { AlertCircle, CheckCircle2, Key, Loader2, Plug, Plus, Settings2, Trash2 } from "lucide-react";
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

const CreateSourceDialog = (): React.ReactElement => {
  const createSource = useCreateSource();
  const [name, setName] = useState("");
  const [sourceType, setSourceType] = useState("github");
  const [open, setOpen] = useState(false);

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    createSource.mutate(
      { sourceType, name },
      {
        onSuccess: () => {
          setOpen(false);
          setName("");
          setSourceType("github");
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button />}>
        <Plus className="size-4" />
        Add Source
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>Add Source</DialogTitle>
            <DialogDescription>Connect a new data source to Prism.</DialogDescription>
          </DialogHeader>

          <div className="mt-4 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="source-name">Name</Label>
              <Input
                id="source-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. canonical/ubuntu"
                required
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="source-type">Type</Label>
              <select
                id="source-type"
                value={sourceType}
                onChange={(e) => setSourceType(e.target.value)}
                className="flex h-8 w-full rounded-lg border border-input bg-transparent px-2.5 py-1 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
              >
                {SOURCE_TYPES.map((t) => (
                  <option key={t.value} value={t.value}>
                    {t.label}
                  </option>
                ))}
              </select>
            </div>

            {createSource.isError && (
              <Alert variant="destructive">
                {createSource.error instanceof Error ? createSource.error.message : "Failed to create source"}
              </Alert>
            )}
          </div>

          <DialogFooter className="mt-4">
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={createSource.isPending || !name.trim()}>
              {createSource.isPending ? "Creating..." : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};

const SetSecretDialog = ({
  source,
  open,
  onOpenChange,
}: {
  source: SourceConfig;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const setSecret = useSetSecret();
  const secretKeys = SECRET_KEYS_BY_TYPE[source.sourceType] ?? ["api_token"];
  const [selectedKey, setSelectedKey] = useState(secretKeys[0] ?? "api_token");
  const [secretValue, setSecretValue] = useState("");

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    setSecret.mutate(
      { sourceId: source.id, secretKey: selectedKey, secretValue },
      {
        onSuccess: () => {
          onOpenChange(false);
          setSecretValue("");
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>Set Secret</DialogTitle>
            <DialogDescription>{source.name}</DialogDescription>
          </DialogHeader>

          <div className="mt-4 space-y-4">
            {secretKeys.length > 1 && (
              <div className="space-y-2">
                <Label htmlFor="secret-key">Secret Key</Label>
                <select
                  id="secret-key"
                  value={selectedKey}
                  onChange={(e) => setSelectedKey(e.target.value)}
                  className="flex h-8 w-full rounded-lg border border-input bg-transparent px-2.5 py-1 text-sm outline-none focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
                >
                  {secretKeys.map((k) => (
                    <option key={k} value={k}>
                      {k}
                    </option>
                  ))}
                </select>
              </div>
            )}

            <div className="space-y-2">
              <Label htmlFor="secret-value">{secretKeys.length <= 1 ? `Value (${selectedKey})` : "Value"}</Label>
              <Input
                id="secret-value"
                type="password"
                value={secretValue}
                onChange={(e) => setSecretValue(e.target.value)}
                placeholder="Paste your token here"
                className="font-mono"
                required
              />
            </div>

            {setSecret.isError && (
              <Alert variant="destructive">
                {setSecret.error instanceof Error ? setSecret.error.message : "Failed to set secret"}
              </Alert>
            )}
          </div>

          <DialogFooter className="mt-4">
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={setSecret.isPending || !secretValue.trim()}>
              {setSecret.isPending ? "Saving..." : "Save Secret"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};

const SourceRow = ({ source }: { source: SourceConfig }): React.ReactElement => {
  const updateSource = useUpdateSource();
  const deleteSource = useDeleteSource();
  const testConnection = useTestConnection();
  const [showSecret, setShowSecret] = useState(false);

  const secretEntries = Object.entries(source.secretStatus);
  const allSecretsSet = secretEntries.length > 0 && secretEntries.every(([, set]) => set);
  const sourceLabel = SOURCE_TYPES.find((t) => t.value === source.sourceType)?.label ?? source.sourceType;

  const handleToggleEnabled = (): void => {
    updateSource.mutate({ sourceId: source.id, enabled: !source.enabled });
  };

  const handleDelete = (): void => {
    if (confirm(`Delete source "${source.name}"?`)) {
      deleteSource.mutate(source.id);
    }
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
              <Badge variant="secondary">{sourceLabel}</Badge>
              {allSecretsSet ? (
                <span className="flex items-center gap-1 text-xs text-green-600">
                  <Key className="size-3" /> Configured
                </span>
              ) : (
                <span className="flex items-center gap-1 text-xs text-amber-600">
                  <Key className="size-3" /> Needs secret
                </span>
              )}
            </div>
          </div>
        </div>

        <div className="flex items-center gap-1">
          <Button variant="ghost" size="icon-sm" onClick={() => setShowSecret(true)} title="Set secret">
            <Settings2 className="size-4" />
          </Button>

          <Button
            variant="ghost"
            size="icon-sm"
            onClick={() => testConnection.mutate(source.id)}
            disabled={testConnection.isPending}
            title="Test connection"
          >
            {testConnection.isPending ? <Loader2 className="size-4 animate-spin" /> : <Plug className="size-4" />}
          </Button>

          <Button
            variant="ghost"
            size="icon-sm"
            onClick={handleDelete}
            disabled={deleteSource.isPending}
            title="Delete source"
            className="hover:text-destructive"
          >
            <Trash2 className="size-4" />
          </Button>
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
              <CheckCircle2 className="size-4" /> Connection successful
            </p>
          ) : (
            <p className="flex items-center gap-1 text-sm text-red-700 dark:text-red-300">
              <AlertCircle className="size-4" /> {testConnection.data.errorMessage || "Connection failed"}
            </p>
          )}
        </div>
      )}

      {testConnection.isError && (
        <div className="mx-4 -mt-1 mb-1 rounded-b border border-t-0 border-red-200 bg-red-50 px-4 py-2 dark:border-red-900 dark:bg-red-950">
          <p className="flex items-center gap-1 text-sm text-red-700 dark:text-red-300">
            <AlertCircle className="size-4" />{" "}
            {testConnection.error instanceof Error ? testConnection.error.message : "Test failed"}
          </p>
        </div>
      )}

      <SetSecretDialog source={source} open={showSecret} onOpenChange={setShowSecret} />
    </>
  );
};

const SourcesTab = (): React.ReactElement => {
  const { data: sources, isLoading, error } = useListSources();

  return (
    <div>
      {isLoading && <p className="text-sm text-muted-foreground">Loading sources...</p>}

      {error && (
        <Alert variant="destructive">
          <AlertCircle className="size-4" />
          Failed to load sources.
        </Alert>
      )}

      {sources && sources.length === 0 && (
        <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
          <Plug className="mb-3 size-10 text-muted-foreground" />
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
    </div>
  );
};

const ApiTokensTab = (): React.ReactElement => (
  <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
    <Key className="mb-3 size-10 text-muted-foreground" />
    <p className="mb-1 font-medium">API Tokens</p>
    <p className="text-sm text-muted-foreground">API token management will be implemented in a future workstream.</p>
  </div>
);

const AdminPage = (): React.ReactElement => {
  return (
    <>
      <PageHeader title="Admin" description="Manage sources and platform settings" actions={<CreateSourceDialog />} />
      <div className="flex-1 p-6">
        <Tabs defaultValue="sources">
          <TabsList>
            <TabsTrigger value="sources">
              <Plug className="size-4" />
              Sources
            </TabsTrigger>
            <TabsTrigger value="tokens">
              <Key className="size-4" />
              API Tokens
            </TabsTrigger>
          </TabsList>
          <TabsContent value="sources">
            <SourcesTab />
          </TabsContent>
          <TabsContent value="tokens">
            <ApiTokensTab />
          </TabsContent>
        </Tabs>
      </div>
    </>
  );
};

export default AdminPage;
