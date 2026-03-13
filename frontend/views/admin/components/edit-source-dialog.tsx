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
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import { X } from "lucide-react";
import { useEffect, useState } from "react";

import type { JsonObject } from "@bufbuild/protobuf";
import type { SourceConfig } from "@ps/api/gen/prism/v1/config_pb";
import { useSetSecret, useUpdateSource } from "@ps/hooks/use-config";
import { cn } from "@ps/cn";

import { SECRET_KEYS_BY_TYPE } from "@/views/admin/lib/source-types";

const toStringArray = (val: unknown): string[] => {
  if (!Array.isArray(val)) return [];
  return val.filter((v): v is string => typeof v === "string");
};

// --- GitHub settings form ---

const GitHubSettingsForm = ({
  settings,
  onChange,
}: {
  settings: JsonObject;
  onChange: (settings: JsonObject) => void;
}): React.ReactElement => {
  const orgs = toStringArray(settings.orgs);
  const baseUrl = typeof settings.base_url === "string" ? settings.base_url : "";
  const apiMode = typeof settings.api_mode === "string" ? settings.api_mode : "rest+graphql";
  const excludeArchived = settings.exclude_archived !== false; // default true
  const excludeRepos = toStringArray(settings.exclude_repos);
  const [orgInput, setOrgInput] = useState("");
  const [excludeInput, setExcludeInput] = useState("");

  const update = (patch: JsonObject): void => {
    onChange({ ...settings, ...patch });
  };

  const addOrg = (): void => {
    const trimmed = orgInput.trim();
    if (trimmed && !orgs.includes(trimmed)) {
      update({ orgs: [...orgs, trimmed] });
      setOrgInput("");
    }
  };

  const removeOrg = (org: string): void => {
    update({ orgs: orgs.filter((o) => o !== org) });
  };

  const addExcludeRepo = (): void => {
    const trimmed = excludeInput.trim();
    if (trimmed && !excludeRepos.includes(trimmed)) {
      update({ exclude_repos: [...excludeRepos, trimmed] });
      setExcludeInput("");
    }
  };

  const removeExcludeRepo = (repo: string): void => {
    update({ exclude_repos: excludeRepos.filter((r) => r !== repo) });
  };

  return (
    <div className="space-y-4">
      {/* Orgs */}
      <div className="space-y-2">
        <Label>
          Organisations <span className="text-destructive">*</span>
        </Label>
        <p className="text-xs text-muted-foreground">
          GitHub organisations to discover repos from.
        </p>
        <div className="flex gap-2">
          <Input
            value={orgInput}
            onChange={(e) => setOrgInput(e.target.value)}
            placeholder="e.g. canonical"
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addOrg();
              }
            }}
          />
          <Button type="button" variant="outline" size="sm" onClick={addOrg}>
            Add
          </Button>
        </div>
        {orgs.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {orgs.map((org) => (
              <Badge key={org} variant="secondary" className="gap-1">
                {org}
                <button
                  type="button"
                  onClick={() => removeOrg(org)}
                  className="hover:text-destructive"
                >
                  <X className="size-3" />
                </button>
              </Badge>
            ))}
          </div>
        )}
      </div>

      {/* Base URL */}
      <div className="space-y-2">
        <Label htmlFor="base-url">API Base URL</Label>
        <Input
          id="base-url"
          value={baseUrl}
          onChange={(e) => update({ base_url: e.target.value || null })}
          placeholder="https://api.github.com"
        />
        <p className="text-xs text-muted-foreground">
          Leave blank for github.com. Set for GitHub Enterprise.
        </p>
      </div>

      {/* API Mode */}
      <div className="space-y-2">
        <Label>API Mode</Label>
        <Select value={apiMode} onValueChange={(v) => v !== null && update({ api_mode: v })}>
          <SelectTrigger className="w-full">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="rest+graphql">REST + GraphQL (recommended)</SelectItem>
            <SelectItem value="rest">REST only</SelectItem>
            <SelectItem value="graphql">GraphQL only</SelectItem>
          </SelectContent>
        </Select>
      </div>

      {/* Exclude archived */}
      <div className="flex items-center justify-between">
        <div>
          <Label>Exclude archived repos</Label>
          <p className="text-xs text-muted-foreground">
            Skip archived repositories during discovery.
          </p>
        </div>
        <button
          type="button"
          onClick={() => update({ exclude_archived: !excludeArchived })}
          className={cn(
            "relative h-5 w-9 rounded-full transition-colors",
            excludeArchived ? "bg-green-500" : "bg-muted",
          )}
        >
          <span
            className={cn(
              "absolute top-0.5 h-4 w-4 rounded-full bg-white transition-transform",
              excludeArchived ? "left-[18px]" : "left-0.5",
            )}
          />
        </button>
      </div>

      {/* Exclude repos */}
      <div className="space-y-2">
        <Label>Exclude repos</Label>
        <p className="text-xs text-muted-foreground">
          Specific repos to skip (e.g. forks, mirrors).
        </p>
        <div className="flex gap-2">
          <Input
            value={excludeInput}
            onChange={(e) => setExcludeInput(e.target.value)}
            placeholder="e.g. org/repo-name"
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addExcludeRepo();
              }
            }}
          />
          <Button type="button" variant="outline" size="sm" onClick={addExcludeRepo}>
            Add
          </Button>
        </div>
        {excludeRepos.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {excludeRepos.map((repo) => (
              <Badge key={repo} variant="secondary" className="gap-1">
                {repo}
                <button
                  type="button"
                  onClick={() => removeExcludeRepo(repo)}
                  className="hover:text-destructive"
                >
                  <X className="size-3" />
                </button>
              </Badge>
            ))}
          </div>
        )}
      </div>
    </div>
  );
};

// --- Secret form (inline, not a separate dialog) ---

const SecretForm = ({ source }: { source: SourceConfig }): React.ReactElement => {
  const setSecret = useSetSecret();
  const secretKeys = SECRET_KEYS_BY_TYPE[source.sourceType] ?? ["api_token"];
  const [selectedKey, setSelectedKey] = useState(secretKeys[0] ?? "api_token");
  const [secretValue, setSecretValue] = useState("");

  const handleSave = (): void => {
    setSecret.mutate(
      { sourceId: source.id, secretKey: selectedKey, secretValue },
      {
        onSuccess: () => {
          setSecretValue("");
        },
      },
    );
  };

  return (
    <div className="space-y-3">
      {secretKeys.length > 1 && (
        <div className="space-y-2">
          <Label>Secret key</Label>
          <Select value={selectedKey} onValueChange={(v) => v !== null && setSelectedKey(v)}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {secretKeys.map((k) => (
                <SelectItem key={k} value={k}>
                  {k}
                  {source.secretStatus[k] ? " (set)" : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>
      )}

      <div className="space-y-2">
        <Label>
          {secretKeys.length <= 1 ? selectedKey : "Value"}
          {source.secretStatus[selectedKey] && (
            <Badge variant="secondary" className="ml-2">
              set
            </Badge>
          )}
        </Label>
        <div className="flex gap-2">
          <Input
            type="password"
            value={secretValue}
            onChange={(e) => setSecretValue(e.target.value)}
            placeholder="Paste new value to update"
            className="font-mono"
          />
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={handleSave}
            disabled={setSecret.isPending || !secretValue.trim()}
          >
            {setSecret.isPending ? "Saving..." : "Save"}
          </Button>
        </div>
      </div>

      {setSecret.isError && (
        <Alert variant="destructive">
          {setSecret.error instanceof Error ? setSecret.error.message : "Failed to set secret"}
        </Alert>
      )}
    </div>
  );
};

// --- Main dialog ---

const settingsForms: Record<
  string,
  (props: { settings: JsonObject; onChange: (s: JsonObject) => void }) => React.ReactElement
> = {
  github: GitHubSettingsForm,
};

export const EditSourceDialog = ({
  source,
  open,
  onOpenChange,
}: {
  source: SourceConfig;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const updateSource = useUpdateSource();
  const [settings, setSettings] = useState<JsonObject>({});

  // Sync settings from source when dialog opens
  useEffect(() => {
    if (open) {
      setSettings(source.settings ?? {});
    }
  }, [open, source.settings]);

  const SettingsForm = settingsForms[source.sourceType];

  const handleSave = (): void => {
    updateSource.mutate(
      { sourceId: source.id, settings },
      {
        onSuccess: () => {
          onOpenChange(false);
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{source.name}</DialogTitle>
          <DialogDescription>Configure source settings and credentials.</DialogDescription>
        </DialogHeader>

        <div className="max-h-[60vh] space-y-4 overflow-y-auto">
          {SettingsForm ? (
            <SettingsForm settings={settings} onChange={setSettings} />
          ) : (
            <p className="text-sm text-muted-foreground">
              No configurable settings for this source type.
            </p>
          )}

          <Separator />

          <div>
            <p className="mb-3 text-sm font-medium">Credentials</p>
            <SecretForm source={source} />
          </div>
        </div>

        {updateSource.isError && (
          <Alert variant="destructive">
            {updateSource.error instanceof Error
              ? updateSource.error.message
              : "Failed to update source"}
          </Alert>
        )}

        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button onClick={handleSave} disabled={updateSource.isPending}>
            {updateSource.isPending ? "Saving..." : "Save Settings"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
