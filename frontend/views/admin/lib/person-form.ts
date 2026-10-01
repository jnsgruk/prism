import { platformLabel } from "@/lib/proto-display";
import type { MessageInitShape } from "@bufbuild/protobuf";
import { z } from "zod";

import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { IdentityInputSchema, Person, PlatformIdentity } from "@ps/api/gen/canonical/prism/v1/org_pb";

export type AccountDraft = {
  key: string;
  id?: string;
  platform: Platform;
  username: string;
  platformInstance: string;
  platformUserId: string;
};

export type PersonDraft = {
  name: string;
  email: string;
  level: string;
  teamId: string;
  accounts: AccountDraft[];
};

export const editablePlatforms = [Platform.GITHUB, Platform.JIRA, Platform.DISCOURSE];

export const accountDraft = (identity: PlatformIdentity): AccountDraft => ({
  key: identity.id,
  id: identity.id,
  platform: identity.platform,
  username: identity.username,
  platformInstance: identity.platformInstance ?? "",
  platformUserId: identity.platformUserId ?? "",
});

export const personDraft = (person?: Person): PersonDraft => ({
  name: person?.name ?? "",
  email: person?.email ?? "",
  level: person?.level ?? "",
  teamId: person?.teamId ?? "",
  accounts: person?.identities.map(accountDraft) ?? [],
});

const schema = z.object({
  name: z.string().trim().min(1, "Enter a name."),
  email: z.union([z.literal(""), z.email("Enter a valid email address.")]),
  level: z.string().trim(),
  teamId: z.string(),
  accounts: z
    .array(
      z.object({
        key: z.string(),
        id: z.string().optional(),
        platform: z.enum(Platform),
        username: z.string().trim(),
        platformInstance: z.string(),
        platformUserId: z.string().trim(),
      }),
    )
    .superRefine((accounts, ctx) => {
      const seen = new Set<string>();

      accounts.forEach((account, index) => {
        if (!editablePlatforms.includes(account.platform)) return;

        if (!account.username)
          ctx.addIssue({
            code: "custom",
            message: `Account ${index + 1}: enter a username or display name.`,
            path: [index, "username"],
          });

        if (account.platform === Platform.DISCOURSE && !account.platformInstance)
          ctx.addIssue({
            code: "custom",
            message: `Account ${index + 1}: select a Discourse instance.`,
            path: [index, "platformInstance"],
          });

        if (
          account.platform === Platform.DISCOURSE &&
          account.username &&
          !/^[a-zA-Z0-9_.-]{1,60}$/.test(account.username)
        )
          ctx.addIssue({
            code: "custom",
            message: `${platformLabel(account.platform, account.platformInstance || undefined)} account ${index + 1}: usernames must contain 1–60 letters, numbers, dots, hyphens, or underscores.`,
            path: [index, "username"],
          });

        if (account.platform === Platform.JIRA && !account.id && !account.platformUserId)
          ctx.addIssue({
            code: "custom",
            message: `Account ${index + 1}: select a Jira result or enter its opaque account ID.`,
            path: [index, "platformUserId"],
          });

        const ownershipKey = `${account.platform}:${account.platformInstance}:${account.platform === Platform.JIRA ? account.platformUserId || account.username : account.username.toLowerCase()}`;

        if (seen.has(ownershipKey))
          ctx.addIssue({
            code: "custom",
            message: `Account ${index + 1}: this account is already in the form.`,
            path: [index],
          });

        seen.add(ownershipKey);
      });
    }),
});

export const validatePersonDraft = (draft: PersonDraft): string | undefined => {
  const result = schema.safeParse({ ...draft, email: draft.email.trim() });

  return result.success ? undefined : result.error.issues[0]?.message;
};

export const identityInput = (account: AccountDraft): MessageInitShape<typeof IdentityInputSchema> => ({
  platform: account.platform,
  username: account.username.trim(),
  platformInstance: account.platformInstance || undefined,
  platformUserId: account.platformUserId.trim() || undefined,
});
