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
  rate_limit_reset_at?: string;
  status_message?: string;
};

export type NormalisedProgress = {
  /** 0–100, or null when indeterminate */
  percent: number | null;
  /** Short context label for the progress column */
  label: string;
  /** Human-readable pause message shown when pipeline is sleeping for rate limit */
  pauseNote?: string;
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

/** Append pause info when the pipeline is sleeping for a rate limit reset. */
const withPauseInfo = (result: NormalisedProgress, progress: RunProgress): NormalisedProgress => {
  if (!progress.rate_limit_reset_at) return result;
  const resetAt = new Date(progress.rate_limit_reset_at);
  const diffMs = resetAt.getTime() - Date.now();
  if (diffMs <= 0) return result;
  const diffMin = Math.ceil(diffMs / 60_000);
  result.pauseNote = diffMin <= 1 ? "Paused — resumes in <1m" : `Paused — resumes in ${String(diffMin)}m`;
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
          return withPauseInfo({ percent: Math.round((done / total) * 90), label: `${done}/${total} repos` }, progress);
        }
        return withPauseInfo({ percent: null, label: "Fetching repos" }, progress);
      }
      case "member_search": {
        const total = progress.search_users_total ?? 0;
        const done = progress.search_users_completed ?? 0;
        if (total > 0) {
          return withPauseInfo(
            { percent: 90 + Math.round((done / total) * 10), label: `${done}/${total} members` },
            progress,
          );
        }
        return withPauseInfo({ percent: null, label: "Searching members" }, progress);
      }
      case "complete":
        return withPauseInfo({ percent: 100, label: "Finalising" }, progress);
      default:
        return withPauseInfo({ percent: null, label: progress.status_message ?? "Starting" }, progress);
    }
  }

  // Generic handler — use status_message if available
  if (progress.status_message) {
    return withPauseInfo({ percent: null, label: progress.status_message }, progress);
  }

  return withPauseInfo({ percent: null, label: "Collecting" }, progress);
};
