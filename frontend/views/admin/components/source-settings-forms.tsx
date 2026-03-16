import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { X } from "lucide-react";
import { useState } from "react";

import type { JsonObject } from "@bufbuild/protobuf";
import { cn } from "@ps/cn";

const toStringArray = (val: unknown): string[] => {
  if (!Array.isArray(val)) return [];
  return val.filter((v): v is string => typeof v === "string");
};

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
      {/* Scope hint */}
      <div className="rounded-md border border-blue-200 bg-blue-50 px-3 py-2 text-xs text-blue-800 dark:border-blue-800 dark:bg-blue-950 dark:text-blue-200">
        Your Personal Access Token needs the <code className="font-semibold">repo</code> and{" "}
        <code className="font-semibold">read:org</code> scopes. The <code>read:org</code> scope
        enables team discovery and team-scoped ingestion.
      </div>

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

const JiraSettingsForm = ({
  settings,
  onChange,
}: {
  settings: JsonObject;
  onChange: (settings: JsonObject) => void;
}): React.ReactElement => {
  const baseUrl = typeof settings.base_url === "string" ? settings.base_url : "";
  const projects = toStringArray(settings.projects);
  const storyPointsField =
    typeof settings.story_points_field === "string" ? settings.story_points_field : "";
  const apiMode = typeof settings.api_mode === "string" ? settings.api_mode : "cloud";
  const [projectInput, setProjectInput] = useState("");

  const update = (patch: JsonObject): void => {
    onChange({ ...settings, ...patch });
  };

  const addProject = (): void => {
    const trimmed = projectInput.trim().toUpperCase();
    if (trimmed && !projects.includes(trimmed)) {
      update({ projects: [...projects, trimmed] });
      setProjectInput("");
    }
  };

  const removeProject = (project: string): void => {
    update({ projects: projects.filter((p) => p !== project) });
  };

  return (
    <div className="space-y-4">
      {/* Credentials hint */}
      <div className="rounded-md border border-blue-200 bg-blue-50 px-3 py-2 text-xs text-blue-800 dark:border-blue-800 dark:bg-blue-950 dark:text-blue-200">
        For Jira Cloud, set the <code className="font-semibold">email</code> and{" "}
        <code className="font-semibold">api_token</code> secrets. For Jira Server/Data Center, only
        the <code className="font-semibold">api_token</code> (PAT) is needed.
      </div>

      {/* Base URL */}
      <div className="space-y-2">
        <Label htmlFor="jira-base-url">
          Base URL <span className="text-destructive">*</span>
        </Label>
        <Input
          id="jira-base-url"
          value={baseUrl}
          onChange={(e) => update({ base_url: e.target.value })}
          placeholder="https://your-org.atlassian.net"
        />
        <p className="text-xs text-muted-foreground">
          The base URL of your Jira instance (Cloud or Server).
        </p>
      </div>

      {/* Project keys */}
      <div className="space-y-2">
        <Label>Project Keys</Label>
        <p className="text-xs text-muted-foreground">
          Optional filter. Leave empty to ingest all projects.
        </p>
        <div className="flex gap-2">
          <Input
            value={projectInput}
            onChange={(e) => setProjectInput(e.target.value)}
            placeholder="e.g. PROJ"
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addProject();
              }
            }}
          />
          <Button type="button" variant="outline" size="sm" onClick={addProject}>
            Add
          </Button>
        </div>
        {projects.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {projects.map((project) => (
              <Badge key={project} variant="secondary" className="gap-1">
                {project}
                <button
                  type="button"
                  onClick={() => removeProject(project)}
                  className="hover:text-destructive"
                >
                  <X className="size-3" />
                </button>
              </Badge>
            ))}
          </div>
        )}
      </div>

      {/* Story Points Field */}
      <div className="space-y-2">
        <Label htmlFor="story-points-field">Story Points Field</Label>
        <Input
          id="story-points-field"
          value={storyPointsField}
          onChange={(e) => update({ story_points_field: e.target.value || null })}
          placeholder="e.g. customfield_10016"
        />
        <p className="text-xs text-muted-foreground">
          Custom field ID for story points. Varies per instance.
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
            <SelectItem value="cloud">Cloud (API token + email)</SelectItem>
            <SelectItem value="server">Server / Data Center (PAT)</SelectItem>
          </SelectContent>
        </Select>
      </div>
    </div>
  );
};

const DiscourseSettingsForm = ({
  settings,
  onChange,
}: {
  settings: JsonObject;
  onChange: (settings: JsonObject) => void;
}): React.ReactElement => {
  const baseUrl = typeof settings.base_url === "string" ? settings.base_url : "";
  const categories = toStringArray(settings.categories);
  const minPosts = typeof settings.min_posts === "number" ? settings.min_posts : 0;
  const [categoryInput, setCategoryInput] = useState("");

  const update = (patch: JsonObject): void => {
    onChange({ ...settings, ...patch });
  };

  const addCategory = (): void => {
    const trimmed = categoryInput.trim().toLowerCase();
    if (trimmed && !categories.includes(trimmed)) {
      update({ categories: [...categories, trimmed] });
      setCategoryInput("");
    }
  };

  const removeCategory = (cat: string): void => {
    update({ categories: categories.filter((c) => c !== cat) });
  };

  return (
    <div className="space-y-4">
      {/* Credentials hint */}
      <div className="rounded-md border border-blue-200 bg-blue-50 px-3 py-2 text-xs text-blue-800 dark:border-blue-800 dark:bg-blue-950 dark:text-blue-200">
        Set the <code className="font-semibold">api_key</code> secret with a Discourse API key. The
        key needs read access. Use <code className="font-semibold">system</code> as the API username
        for global keys.
      </div>

      {/* Base URL */}
      <div className="space-y-2">
        <Label htmlFor="discourse-base-url">
          Base URL <span className="text-destructive">*</span>
        </Label>
        <Input
          id="discourse-base-url"
          value={baseUrl}
          onChange={(e) => update({ base_url: e.target.value })}
          placeholder="https://discourse.ubuntu.com"
        />
        <p className="text-xs text-muted-foreground">The base URL of the Discourse instance.</p>
      </div>

      {/* Category filter */}
      <div className="space-y-2">
        <Label>Category Filter</Label>
        <p className="text-xs text-muted-foreground">
          Optional. Leave empty to ingest all categories.
        </p>
        <div className="flex gap-2">
          <Input
            value={categoryInput}
            onChange={(e) => setCategoryInput(e.target.value)}
            placeholder="e.g. development"
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addCategory();
              }
            }}
          />
          <Button type="button" variant="outline" size="sm" onClick={addCategory}>
            Add
          </Button>
        </div>
        {categories.length > 0 && (
          <div className="flex flex-wrap gap-1">
            {categories.map((cat) => (
              <Badge key={cat} variant="secondary" className="gap-1">
                {cat}
                <button
                  type="button"
                  onClick={() => removeCategory(cat)}
                  className="hover:text-destructive"
                >
                  <X className="size-3" />
                </button>
              </Badge>
            ))}
          </div>
        )}
      </div>

      {/* Min posts */}
      <div className="space-y-2">
        <Label htmlFor="min-posts">Minimum Posts</Label>
        <Input
          id="min-posts"
          type="number"
          min={0}
          value={minPosts}
          onChange={(e) => update({ min_posts: Number.parseInt(e.target.value, 10) || 0 })}
        />
        <p className="text-xs text-muted-foreground">
          Skip topics with fewer posts than this. Set to 0 to ingest all.
        </p>
      </div>
    </div>
  );
};

export const settingsForms: Record<
  string,
  (props: { settings: JsonObject; onChange: (s: JsonObject) => void }) => React.ReactElement
> = {
  github: GitHubSettingsForm,
  jira: JiraSettingsForm,
  discourse: DiscourseSettingsForm,
};
