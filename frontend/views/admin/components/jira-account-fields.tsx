import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { useDebouncedValue } from "@/lib/hooks/use-debounced-value";
import { useLookupJiraAccounts } from "@/views/admin/hooks/use-person-management";
import type { AccountDraft } from "@/views/admin/lib/person-form";
import { useState } from "react";

import type { SourceConfig } from "@ps/api/gen/canonical/prism/v1/config_pb";

export const JiraAccountFields = ({
  account,
  sources,
  onChange,
}: {
  account: AccountDraft;
  sources: SourceConfig[];
  onChange: (account: AccountDraft) => void;
}): React.ReactElement => {
  const [sourceId, setSourceId] = useState("");
  const [search, setSearch] = useState("");
  const [startAt, setStartAt] = useState(0);
  const [manuallySupplied, setManuallySupplied] = useState(false);

  const query = useDebouncedValue(search);
  const lookup = useLookupJiraAccounts(sourceId, query, startAt);

  const enabledSources = sources.filter((source) => source.enabled);
  const prefix = `account-${account.key}`;

  return (
    <div className="min-w-0 space-y-3">
      <div className="space-y-2">
        <Label htmlFor={`${prefix}-source`}>Jira lookup source</Label>
        <Select
          value={sourceId}
          onValueChange={(value) => {
            setSourceId(value ?? "");
            setStartAt(0);
          }}
        >
          <SelectTrigger id={`${prefix}-source`} className="w-full min-w-0">
            <SelectValue>
              {enabledSources.find((source) => source.id === sourceId)?.name ?? "Choose a source"}
            </SelectValue>
          </SelectTrigger>
          <SelectContent>
            {enabledSources.map((source) => (
              <SelectItem key={source.id} value={source.id}>
                {source.name}
              </SelectItem>
            ))}
          </SelectContent>
        </Select>
      </div>
      <div className="space-y-2">
        <Label htmlFor={`${prefix}-search`}>Search Jira accounts</Label>
        <Input
          id={`${prefix}-search`}
          value={search}
          disabled={!sourceId}
          placeholder="Name or email (at least 2 characters)"
          onChange={(e) => {
            setSearch(e.target.value);
            setStartAt(0);
          }}
        />
      </div>
      {sourceId && query.trim().length >= 2 && (
        <div className="min-w-0 space-y-2" aria-live="polite">
          {lookup.isFetching && <p className="text-sm text-muted-foreground">Searching...</p>}
          {lookup.error && (
            <Alert variant="destructive" className="min-w-0 break-words">
              {lookup.error.message} Use the account ID fields below or correct the source configuration.
            </Alert>
          )}
          {lookup.data?.accounts.length === 0 && (
            <p className="text-sm text-muted-foreground">
              No matching accounts. Try another search or enter the account ID below.
            </p>
          )}
          {!lookup.isFetching &&
            lookup.data?.accounts.map((candidate) => (
              <Button
                key={candidate.accountId}
                type="button"
                variant={account.platformUserId === candidate.accountId ? "secondary" : "outline"}
                className="h-auto w-full min-w-0 justify-start whitespace-normal text-left"
                onClick={() => {
                  setManuallySupplied(false);
                  onChange({ ...account, username: candidate.displayName, platformUserId: candidate.accountId });
                }}
              >
                <span className="min-w-0 break-words">
                  <span className="block font-medium">
                    {candidate.displayName}
                    {!candidate.active && " (inactive)"}
                  </span>
                  <span className="block text-xs text-muted-foreground">
                    {candidate.email ?? "Email hidden"} · {candidate.accountId}
                  </span>
                </span>
              </Button>
            ))}
          <div className="flex gap-2">
            {startAt > 0 && (
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={lookup.isFetching}
                onClick={() => setStartAt(Math.max(0, startAt - 20))}
              >
                Previous results
              </Button>
            )}
            {lookup.data?.nextStartAt !== undefined && (
              <Button
                type="button"
                size="sm"
                variant="outline"
                disabled={lookup.isFetching}
                onClick={() => setStartAt(lookup.data?.nextStartAt ?? 0)}
              >
                More results
              </Button>
            )}
          </div>
        </div>
      )}
      <p className="text-xs text-muted-foreground">
        Select a result explicitly, or enter the opaque Jira account ID directly when lookup is unavailable. The display
        name is saved separately.
      </p>
      <div className="space-y-2">
        <Label htmlFor={`${prefix}-id`}>Jira account ID (opaque ID, not email)</Label>
        <Input
          id={`${prefix}-id`}
          value={account.platformUserId}
          onChange={(e) => {
            setManuallySupplied(true);
            onChange({ ...account, platformUserId: e.target.value });
          }}
        />
      </div>
      {manuallySupplied && account.platformUserId && (
        <p className="text-xs text-muted-foreground">Manually supplied account ID</p>
      )}
      <div className="space-y-2">
        <Label htmlFor={`${prefix}-username`}>Jira display name / username</Label>
        <Input
          id={`${prefix}-username`}
          value={account.username}
          onChange={(e) => onChange({ ...account, username: e.target.value })}
        />
      </div>
    </div>
  );
};
