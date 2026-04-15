/**
 * Normalises the heterogeneous progressJson shapes from different ingestion
 * handlers into a common { percent, label } that the UI can render uniformly.
 */

export type RunProgress = {
  phase?: string;
  repos_total?: number;
  repos_completed?: number;
  current_repo?: string;
  prs_fetched?: number;
  reviews_fetched?: number;
  identities_skipped?: number;
  search_users_total?: number;
  search_users_completed?: number;
  rate_limit_remaining?: number;
  rate_limit_limit?: number;
  status_message?: string;
};

export type NormalisedProgress = {
  /** 0–100, or null when indeterminate */
  percent: number | null;
  /** Short context label for the progress column */
  label: string;
  /** Inline rate-limit note, e.g. "4,334/5,000 API calls left" */
  rateLimitNote?: string;
  /** True when remaining / limit < 0.1 */
  rateLimitLow?: boolean;
};

const isRunProgress = (v: unknown): v is RunProgress => typeof v === "object" && v !== null;

/** Parse the raw progressJson string into a typed object. Returns null on failure. */
export const parseProgress = (json: string | undefined): RunProgress | null => {
  if (!json) return null;
  try {
    const parsed: unknown = JSON.parse(json);
    return isRunProgress(parsed) ? parsed : null;
  } catch {
    return null;
  }
};

/** Append rate-limit fields to a result when the progress carries them. */
const withRateLimit = (result: NormalisedProgress, progress: RunProgress): NormalisedProgress => {
  if (progress.rate_limit_limit && progress.rate_limit_limit > 0) {
    const remaining = progress.rate_limit_remaining ?? 0;
    const pct = Math.round((remaining / progress.rate_limit_limit) * 100);
    result.rateLimitNote = `${pct}% API calls left`;
    result.rateLimitLow = remaining / progress.rate_limit_limit < 0.1;
  }
  return result;
};

/** Normalise handler-specific progress into a uniform { percent, label }. */
export const normaliseProgress = (sourceType: string, progress: RunProgress | null): NormalisedProgress => {
  if (!progress) return { percent: null, label: "Starting" };

  // GitHub has structured phases — weighted so the bar never resets:
  //   team_repos  → 0-90%
  //   member_search → 90-100%
  if (sourceType === "github") {
    switch (progress.phase) {
      case "team_repos": {
        const total = progress.repos_total ?? 0;
        const done = progress.repos_completed ?? 0;
        if (total > 0) {
          return withRateLimit({ percent: Math.round((done / total) * 90), label: `${done}/${total} repos` }, progress);
        }
        return withRateLimit({ percent: null, label: "Fetching repos" }, progress);
      }
      case "member_search": {
        const total = progress.search_users_total ?? 0;
        const done = progress.search_users_completed ?? 0;
        if (total > 0) {
          return withRateLimit(
            { percent: 90 + Math.round((done / total) * 10), label: `${done}/${total} members` },
            progress,
          );
        }
        return withRateLimit({ percent: null, label: "Searching members" }, progress);
      }
      case "complete":
        return withRateLimit({ percent: 100, label: "Finalising" }, progress);
      default:
        return withRateLimit({ percent: null, label: progress.status_message ?? "Starting" }, progress);
    }
  }

  // Generic handler — use status_message if available
  if (progress.status_message) {
    return withRateLimit({ percent: null, label: progress.status_message }, progress);
  }

  return withRateLimit({ percent: null, label: "Collecting" }, progress);
};
