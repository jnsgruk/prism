import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { platformLabel } from "@/lib/proto-display";
import { JiraAccountFields } from "@/views/admin/components/jira-account-fields";
import { editablePlatforms, type AccountDraft } from "@/views/admin/lib/person-form";
import { Plus, Trash2 } from "lucide-react";

import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { SourceConfig } from "@ps/api/gen/canonical/prism/v1/config_pb";

export const PersonAccounts = ({
  accounts,
  onChange,
  sources,
  sourceError,
}: {
  accounts: AccountDraft[];
  onChange: (accounts: AccountDraft[]) => void;
  sources: SourceConfig[];
  sourceError?: Error | null;
}): React.ReactElement => {
  const update = (changed: AccountDraft): void =>
    onChange(accounts.map((account) => (account.key === changed.key ? changed : account)));

  const discourseSources = sources.filter(
    (source) => source.sourceType === Platform.DISCOURSE && source.enabled && source.platformInstance,
  );
  const instances = [...new Map(discourseSources.map((source) => [source.platformInstance!, source])).values()];

  return (
    <div className="min-w-0 space-y-3">
      <p className="text-sm font-medium">Platform accounts (optional)</p>
      {sourceError && (
        <Alert variant="destructive" className="min-w-0 break-words">
          Could not load sources: {sourceError.message}. Saved accounts are retained.
        </Alert>
      )}
      {accounts.map((account, index) => {
        const editable = editablePlatforms.includes(account.platform);
        const prefix = `account-${account.key}`;

        return (
          <div
            key={account.key}
            className="min-w-0 space-y-3 rounded-md border p-3"
            role="group"
            aria-label={`${platformLabel(account.platform, account.platformInstance || undefined)} account ${index + 1}`}
          >
            <div className="flex min-w-0 items-center justify-between gap-2">
              <p className="min-w-0 break-words text-sm font-medium">
                {platformLabel(account.platform, account.platformInstance || undefined)}
              </p>
              {editable && (
                <Button
                  type="button"
                  variant="ghost"
                  size="icon-sm"
                  aria-label={`Remove account ${index + 1}`}
                  onClick={() => onChange(accounts.filter((row) => row.key !== account.key))}
                >
                  <Trash2 className="size-4" />
                </Button>
              )}
            </div>
            {account.platform === Platform.JIRA ? (
              <JiraAccountFields
                account={account}
                sources={sources.filter((source) => source.sourceType === Platform.JIRA)}
                onChange={update}
              />
            ) : (
              <>
                {account.platform === Platform.DISCOURSE && (
                  <div className="space-y-2">
                    <Label htmlFor={`${prefix}-instance`}>Discourse instance</Label>
                    <Select
                      value={account.platformInstance}
                      disabled={!!account.id}
                      onValueChange={(value) => update({ ...account, platformInstance: value ?? "" })}
                    >
                      <SelectTrigger id={`${prefix}-instance`} className="w-full min-w-0">
                        <SelectValue>{account.platformInstance || "Choose an instance"}</SelectValue>
                      </SelectTrigger>
                      <SelectContent>
                        {account.platformInstance &&
                          !instances.some((source) => source.platformInstance === account.platformInstance) && (
                            <SelectItem value={account.platformInstance}>
                              {account.platformInstance} (saved instance)
                            </SelectItem>
                          )}
                        {instances.map((source) => (
                          <SelectItem key={source.platformInstance} value={source.platformInstance!}>
                            {source.name} ({source.platformInstance})
                          </SelectItem>
                        ))}
                      </SelectContent>
                    </Select>
                    {account.id && (
                      <p className="text-xs text-muted-foreground">
                        To move to another instance, remove this account and add a new one.
                      </p>
                    )}
                  </div>
                )}
                <div className="space-y-2">
                  <Label htmlFor={`${prefix}-username`}>{platformLabel(account.platform)} username</Label>
                  <Input
                    id={`${prefix}-username`}
                    value={account.username}
                    disabled={!editable}
                    onChange={(e) => update({ ...account, username: e.target.value })}
                  />
                </div>
              </>
            )}
          </div>
        );
      })}
      <div className="flex flex-wrap gap-2">
        {editablePlatforms.map((platform) => (
          <Button
            key={platform}
            type="button"
            variant="outline"
            size="sm"
            onClick={() =>
              onChange([
                ...accounts,
                { key: crypto.randomUUID(), platform, username: "", platformInstance: "", platformUserId: "" },
              ])
            }
          >
            <Plus className="size-3.5" />
            Add {platformLabel(platform)} account
          </Button>
        ))}
      </div>
    </div>
  );
};
