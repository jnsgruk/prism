/**
 * Shared timestamp and duration formatting utilities.
 *
 * All formatters accept proto-style `{ seconds: bigint }` timestamps.
 */

type Timestamp = { seconds: bigint } | undefined;

/** Short date + 24h time: "Mar 16 14:30". Returns em-dash for missing values. */
export const formatTimestamp = (ts: Timestamp): string => {
  if (!ts) return "\u2014";
  const date = new Date(Number(ts.seconds) * 1000);
  return (
    date.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
    " " +
    date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false })
  );
};

/** Full locale string: "3/16/2026, 14:30:00". Returns em-dash for missing values. */
export const formatFullTimestamp = (ts: Timestamp): string => {
  if (!ts) return "\u2014";
  return new Date(Number(ts.seconds) * 1000).toLocaleString();
};

/** Date-only format: "Mar 16, 2026". Returns "Never" for missing values. */
export const formatDateOnly = (ts: Timestamp): string => {
  if (!ts) return "Never";
  const date = new Date(Number(ts.seconds) * 1000);
  return date.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
};

/**
 * Duration between two timestamps: "2m 15s" or "8s". Returns em-dash if start is missing.
 *
 * When `end` is missing and `nowMs` is provided, computes elapsed time from `start` to `nowMs`
 * (useful for in-progress work). Without `nowMs`, a missing `end` returns em-dash.
 */
export const formatDuration = (start: Timestamp, end: Timestamp, nowMs?: number): string => {
  if (!start) return "\u2014";
  let endSec: number;
  if (end) {
    endSec = Number(end.seconds);
  } else if (nowMs !== undefined) {
    endSec = Math.floor(nowMs / 1000);
  } else {
    return "\u2014";
  }
  const diffSec = Math.max(0, endSec - Number(start.seconds));
  if (diffSec < 60) return `${String(diffSec)}s`;
  const totalMin = Math.floor(diffSec / 60);
  if (totalMin < 60) return `${String(totalMin)}m ${String(diffSec % 60)}s`;
  const totalHr = Math.floor(totalMin / 60);
  if (totalHr < 24) return `${String(totalHr)}h ${String(totalMin % 60)}m`;
  const days = Math.floor(totalHr / 24);
  return `${String(days)}d ${String(totalHr % 24)}h`;
};

/** Relative time: "5m ago", "2h ago", "1d ago". Falls back to `formatDateOnly` after 30 days. */
export const formatRelativeTime = (ts: Timestamp): string => {
  if (!ts) return "Never";
  const now = Date.now();
  const then = Number(ts.seconds) * 1000;
  const diffMs = now - then;
  const diffMin = Math.floor(diffMs / 60_000);
  if (diffMin < 1) return "just now";
  if (diffMin < 60) return `${String(diffMin)}m ago`;
  const diffHours = Math.floor(diffMin / 60);
  if (diffHours < 24) return `${String(diffHours)}h ago`;
  const diffDays = Math.floor(diffHours / 24);
  if (diffDays < 30) return `${String(diffDays)}d ago`;
  return formatDateOnly(ts);
};

/** Relative time from an ISO 8601 string: "5m ago", "2h ago", "1d ago". */
export const formatRelativeTimeIso = (isoString: string): string => {
  const date = new Date(isoString);
  const now = Date.now();
  const diffMs = now - date.getTime();
  const diffMins = Math.floor(diffMs / 60_000);
  if (diffMins < 1) return "just now";
  if (diffMins < 60) return `${String(diffMins)}m ago`;
  const diffHours = Math.floor(diffMins / 60);
  if (diffHours < 24) return `${String(diffHours)}h ago`;
  const diffDays = Math.floor(diffHours / 24);
  return `${String(diffDays)}d ago`;
};
