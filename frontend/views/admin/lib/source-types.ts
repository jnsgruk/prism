import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { platformKey } from "@/lib/proto-display";

export const SOURCE_TYPES = [
  { value: "github", label: "GitHub", platform: Platform.GITHUB },
  { value: "jira", label: "Jira", platform: Platform.JIRA },
  { value: "discourse", label: "Discourse", platform: Platform.DISCOURSE },
  { value: "launchpad", label: "Launchpad", platform: Platform.LAUNCHPAD },
  { value: "google_drive", label: "Google Drive", platform: Platform.GOOGLE_DRIVE },
  { value: "mailing_list", label: "Mailing List", platform: Platform.MAILING_LIST },
];

export const SECRET_KEYS_BY_TYPE: Record<string, string[]> = {
  [platformKey(Platform.GITHUB)]: ["api_token"],
  [platformKey(Platform.JIRA)]: ["api_token", "email"],
  [platformKey(Platform.DISCOURSE)]: ["api_key"],
  [platformKey(Platform.LAUNCHPAD)]: ["oauth_token"],
  [platformKey(Platform.GOOGLE_DRIVE)]: ["service_account_key"],
  [platformKey(Platform.MAILING_LIST)]: [],
};

/** Normalize a Platform enum to its base key string for UI lookups.
 *  e.g. Platform.GITHUB → "github", Platform.DISCOURSE → "discourse" */
export const baseSourceType = (sourceType: Platform): string => platformKey(sourceType);
