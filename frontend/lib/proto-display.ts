/**
 * Display name mappings for proto enum values.
 *
 * These functions convert numeric proto enum values to human-readable strings
 * for display in the UI.
 */

import {
  AiProvider,
  ContributionState,
  ContributionType,
  EnrichmentType,
  InsightPeriod,
  Platform,
  RunStatus,
} from "@ps/api/gen/canonical/prism/v1/common_pb";

// ---------------------------------------------------------------------------
// Platform
// ---------------------------------------------------------------------------

const PLATFORM_LABELS: Record<Platform, string> = {
  [Platform.UNSPECIFIED]: "",
  [Platform.GITHUB]: "GitHub",
  [Platform.JIRA]: "Jira",
  [Platform.DISCOURSE]: "Discourse",
  [Platform.LAUNCHPAD]: "Launchpad",
  [Platform.MATTERMOST]: "Mattermost",
  [Platform.GOOGLE_DRIVE]: "Google Drive",
  [Platform.MAILING_LIST]: "Mailing List",
};

/** Human-readable platform name. Appends instance if present (e.g. "Discourse (ubuntu)"). */
export const platformLabel = (platform: Platform, instance?: string): string => {
  const base = PLATFORM_LABELS[platform] ?? String(platform);
  return instance ? `${base} (${instance})` : base;
};

/** Lowercase platform key for use in CSS classes, badge text, etc. */
export const platformKey = (platform: Platform, instance?: string): string => {
  const base = PLATFORM_LABELS[platform]?.toLowerCase() ?? "";
  return instance ? `${base}-${instance}` : base;
};

// ---------------------------------------------------------------------------
// ContributionType
// ---------------------------------------------------------------------------

const CONTRIBUTION_TYPE_LABELS: Record<ContributionType, string> = {
  [ContributionType.UNSPECIFIED]: "",
  [ContributionType.PULL_REQUEST]: "Pull Request",
  [ContributionType.PR_REVIEW]: "PR Review",
  [ContributionType.JIRA_TICKET]: "Jira Ticket",
  [ContributionType.DISCOURSE_TOPIC]: "Discourse Topic",
  [ContributionType.DISCOURSE_POST]: "Discourse Post",
  [ContributionType.DISCOURSE_LIKE]: "Discourse Like",
};

export const contributionTypeLabel = (ct: ContributionType): string => CONTRIBUTION_TYPE_LABELS[ct] ?? String(ct);

// ---------------------------------------------------------------------------
// ContributionState
// ---------------------------------------------------------------------------

const CONTRIBUTION_STATE_LABELS: Record<ContributionState, string> = {
  [ContributionState.UNSPECIFIED]: "",
  [ContributionState.OPEN]: "Open",
  [ContributionState.CLOSED]: "Closed",
  [ContributionState.MERGED]: "Merged",
  [ContributionState.IN_PROGRESS]: "In Progress",
  [ContributionState.APPROVED]: "Approved",
  [ContributionState.CHANGES_REQUESTED]: "Changes Requested",
  [ContributionState.COMMENTED]: "Commented",
  [ContributionState.PENDING]: "Pending",
  [ContributionState.DISMISSED]: "Dismissed",
  [ContributionState.DONE]: "Done",
};

export const contributionStateLabel = (cs: ContributionState): string => CONTRIBUTION_STATE_LABELS[cs] ?? String(cs);

// ---------------------------------------------------------------------------
// RunStatus
// ---------------------------------------------------------------------------

const RUN_STATUS_LABELS: Record<RunStatus, string> = {
  [RunStatus.UNSPECIFIED]: "",
  [RunStatus.RUNNING]: "Running",
  [RunStatus.COMPLETED]: "Completed",
  [RunStatus.COMPLETED_WITH_WARNINGS]: "Completed with Warnings",
  [RunStatus.FAILED]: "Failed",
  [RunStatus.CANCELLED]: "Cancelled",
};

export const runStatusLabel = (rs: RunStatus): string => RUN_STATUS_LABELS[rs] ?? String(rs);

// ---------------------------------------------------------------------------
// AiProvider
// ---------------------------------------------------------------------------

const AI_PROVIDER_LABELS: Record<AiProvider, string> = {
  [AiProvider.UNSPECIFIED]: "",
  [AiProvider.GOOGLE]: "Google",
};

export const aiProviderLabel = (p: AiProvider): string => AI_PROVIDER_LABELS[p] ?? String(p);

/** Lowercase key for provider (e.g. "google"). */
export const aiProviderKey = (p: AiProvider): string => AI_PROVIDER_LABELS[p]?.toLowerCase() ?? "";

// ---------------------------------------------------------------------------
// EnrichmentType
// ---------------------------------------------------------------------------

const ENRICHMENT_TYPE_LABELS: Record<EnrichmentType, string> = {
  [EnrichmentType.UNSPECIFIED]: "",
  [EnrichmentType.REVIEW_DEPTH]: "Review Depth",
  [EnrichmentType.SENTIMENT]: "Sentiment",
  [EnrichmentType.SIGNIFICANCE]: "Significance",
  [EnrichmentType.TOPIC]: "Topic",
};

export const enrichmentTypeLabel = (et: EnrichmentType): string => ENRICHMENT_TYPE_LABELS[et] ?? String(et);

/** Lowercase key matching the DB string (e.g. "review_depth", "sentiment"). */
export const enrichmentTypeKey = (et: EnrichmentType): string => {
  switch (et) {
    case EnrichmentType.REVIEW_DEPTH:
      return "review_depth";
    case EnrichmentType.SENTIMENT:
      return "sentiment";
    case EnrichmentType.SIGNIFICANCE:
      return "significance";
    case EnrichmentType.TOPIC:
      return "topic";
    default:
      return "";
  }
};

// ---------------------------------------------------------------------------
// InsightPeriod
// ---------------------------------------------------------------------------

const INSIGHT_PERIOD_LABELS: Record<InsightPeriod, string> = {
  [InsightPeriod.UNSPECIFIED]: "",
  [InsightPeriod.LAST_WEEK]: "Last Week",
  [InsightPeriod.LAST_MONTH]: "Last Month",
  [InsightPeriod.LAST_QUARTER]: "Last Quarter",
  [InsightPeriod.LAST_YEAR]: "Last Year",
};

export const insightPeriodLabel = (p: InsightPeriod): string => INSIGHT_PERIOD_LABELS[p] ?? String(p);
