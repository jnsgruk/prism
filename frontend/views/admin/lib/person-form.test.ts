import { personDraft, validatePersonDraft } from "@/views/admin/lib/person-form";
import { describe, expect, it } from "vite-plus/test";

import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";

describe("Discourse account validation", () => {
  it("identifies the invalid account and instance when a username has a stray backtick", () => {
    const draft = personDraft();
    draft.name = "Graham Morrison";
    draft.accounts = ["ubuntu-discourse", "internal-discourse"].map((platformInstance, index) => ({
      key: platformInstance,
      platform: Platform.DISCOURSE,
      platformInstance,
      username: index === 0 ? "degville" : "degville`",
      platformUserId: "",
    }));

    const error = validatePersonDraft(draft);
    expect(error).toContain("Discourse (internal-discourse) account 2");
    expect(error).toContain("letters, numbers, dots, hyphens, or underscores");

    draft.accounts[1]!.username = "degville";
    expect(validatePersonDraft(draft)).toBeUndefined();
  });
});
