import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { SourceConfig } from "@ps/api/gen/canonical/prism/v1/config_pb";
import type { Person } from "@ps/api/gen/canonical/prism/v1/org_pb";

export const backfillSourceReason = (person: Person, source: SourceConfig): string | undefined => {
  if (!person.active) return "This person is inactive.";
  if (!source.enabled) return "Source is disabled.";
  if (![Platform.GITHUB, Platform.JIRA, Platform.DISCOURSE].includes(source.sourceType))
    return "Person backfills are unsupported for this platform.";
  if (source.sourceType === Platform.DISCOURSE && !source.platformInstance?.trim())
    return "Configure a Discourse instance for this source.";
  if (
    source.sourceType === Platform.JIRA &&
    source.settings?.api_mode !== undefined &&
    source.settings.api_mode !== "cloud"
  )
    return "Only Jira Cloud supports person backfills.";

  const accounts = person.identities.filter(
    (identity) =>
      identity.platform === source.sourceType && (identity.platformInstance ?? "") === (source.platformInstance ?? ""),
  );
  if (!accounts.length) return "No saved account matches this source and instance.";
  if (accounts.length !== 1) return "Choose one saved account for this platform and instance.";
  const account = accounts[0]!;
  if (!account.username.trim()) return "The saved account needs a username.";
  if (source.sourceType === Platform.JIRA && !account.platformUserId?.trim())
    return "The saved Jira account needs an opaque account ID.";
  return undefined;
};

export const todayDate = (): string => {
  return new Date().toISOString().slice(0, 10);
};

export const backfillDateError = (value: string): string | undefined => {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return "Enter a start date in YYYY-MM-DD format.";
  const parsed = new Date(`${value}T00:00:00Z`);
  if (!Number.isFinite(parsed.getTime()) || parsed.toISOString().slice(0, 10) !== value)
    return "Enter a valid start date.";
  if (value > todayDate()) return "The start date cannot be in the future.";
  return undefined;
};
