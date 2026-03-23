/** Format a number of hours as "Xh" or em-dash if zero. */
export const fmtHours = (h: number): string => (h > 0 ? `${h.toFixed(1)}h` : "\u2014");

/** Format a float to one decimal place or em-dash if zero. */
export const fmtFloat = (v: number): string => (v > 0 ? v.toFixed(1) : "\u2014");

/** Format a 0–1 ratio as a percentage string like "42%". */
export const fmtPercent = (v: number): string => `${Math.round(v * 100)}%`;

/** Extract friendly instance name from platform string (e.g. "discourse-ubuntu" -> "Ubuntu").
 *  @deprecated Use platformLabel from proto-display instead */
export const instanceLabel = (platform: string | number): string => {
  const s = String(platform);
  const suffix = s.replace(/^discourse-?/, "");
  if (!suffix) return s;
  return suffix.replace(/-/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
};
