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
};

export type ProgressDetail = {
  /** PR count (GitHub-specific) */
  prsFetched?: number;
  /** Review count (GitHub-specific) */
  reviewsFetched?: number;
  /** Identities skipped (GitHub-specific) */
  identitiesSkipped?: number;
  /** Rate limit remaining / limit */
  rateLimit?: { remaining: number; limit: number };
  /** Full status message from the handler */
  statusMessage?: string;
};

/** Parse the raw progressJson string into a typed object. Returns null on failure. */
export const parseProgress = (json: string | undefined): RunProgress | null => {
  if (!json) return null;
  try {
    return JSON.parse(json) as RunProgress;
  } catch {
    return null;
  }
};

/** Normalise handler-specific progress into a uniform { percent, label }. */
export const normaliseProgress = (
  sourceType: string,
  progress: RunProgress | null,
): NormalisedProgress => {
  if (!progress) return { percent: null, label: "Starting" };

  // GitHub has structured phases
  if (sourceType === "github") {
    switch (progress.phase) {
      case "team_repos": {
        const total = progress.repos_total ?? 0;
        const done = progress.repos_completed ?? 0;
        if (total > 0) {
          return {
            percent: Math.round((done / total) * 100),
            label: `${done}/${total} repos`,
          };
        }
        return { percent: null, label: "Fetching repos" };
      }
      case "member_search": {
        const total = progress.search_users_total ?? 0;
        const done = progress.search_users_completed ?? 0;
        if (total > 0) {
          return {
            percent: Math.round((done / total) * 100),
            label: `${done}/${total} members`,
          };
        }
        return { percent: null, label: "Searching members" };
      }
      case "complete":
        return { percent: 100, label: "Finalising" };
      default:
        return { percent: null, label: progress.status_message ?? "Starting" };
    }
  }

  // Generic handler — use status_message if available
  if (progress.status_message) {
    return { percent: null, label: progress.status_message };
  }

  return { percent: null, label: "Collecting" };
};

/** Extract detail fields for the expandable row. */
export const extractDetail = (progress: RunProgress | null): ProgressDetail | null => {
  if (!progress) return null;

  const detail: ProgressDetail = {};
  let hasAny = false;

  if (progress.prs_fetched && progress.prs_fetched > 0) {
    detail.prsFetched = progress.prs_fetched;
    hasAny = true;
  }
  if (progress.reviews_fetched && progress.reviews_fetched > 0) {
    detail.reviewsFetched = progress.reviews_fetched;
    hasAny = true;
  }
  if (progress.identities_skipped && progress.identities_skipped > 0) {
    detail.identitiesSkipped = progress.identities_skipped;
    hasAny = true;
  }
  if (progress.rate_limit_limit && progress.rate_limit_limit > 0) {
    detail.rateLimit = {
      remaining: progress.rate_limit_remaining ?? 0,
      limit: progress.rate_limit_limit,
    };
    hasAny = true;
  }
  // Only include statusMessage if there are also stats to show —
  // otherwise it just duplicates the progress label in the main row.
  if (hasAny && progress.status_message) {
    detail.statusMessage = progress.status_message;
  }

  return hasAny ? detail : null;
};
