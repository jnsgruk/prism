export const SOURCE_TYPES = [
  { value: "github", label: "GitHub" },
  { value: "jira", label: "Jira" },
  { value: "discourse", label: "Discourse" },
  { value: "launchpad", label: "Launchpad" },
  { value: "google_drive", label: "Google Drive" },
  { value: "mailing_list", label: "Mailing List" },
];

export const SECRET_KEYS_BY_TYPE: Record<string, string[]> = {
  github: ["api_token"],
  jira: ["api_token", "email"],
  discourse: [],
  launchpad: ["oauth_token"],
  google_drive: ["service_account_key"],
  mailing_list: [],
};

/** Normalize an instance-qualified source type to its base type for UI lookups.
 *  e.g. "discourse-ubuntu" → "discourse", "github" → "github" */
export const baseSourceType = (sourceType: string): string =>
  sourceType.startsWith("discourse-") ? "discourse" : sourceType;
