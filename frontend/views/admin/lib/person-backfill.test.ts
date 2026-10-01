import { create } from "@bufbuild/protobuf";
import { describe, expect, it, vi } from "vite-plus/test";

import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { SourceConfigSchema } from "@ps/api/gen/canonical/prism/v1/config_pb";
import { PersonSchema, PlatformIdentitySchema } from "@ps/api/gen/canonical/prism/v1/org_pb";
import { isActivePipeline } from "@ps/pipeline-status";

import { backfillDateError, backfillSourceReason, todayDate } from "./person-backfill";

describe("person backfill eligibility", () => {
  const person = create(PersonSchema, {
    active: true,
    identities: [
      { id: "discourse", platform: Platform.DISCOURSE, platformInstance: "ubuntu", username: "alex" },
      { id: "jira", platform: Platform.JIRA, username: "Alex", platformUserId: "opaque" },
    ],
  });

  it("binds exact Discourse instances and rejects disabled sources", () => {
    const source = create(SourceConfigSchema, {
      enabled: true,
      sourceType: Platform.DISCOURSE,
      platformInstance: "ubuntu",
    });
    expect(backfillSourceReason(person, source)).toBeUndefined();
    expect(
      backfillSourceReason(person, create(SourceConfigSchema, { ...source, platformInstance: "snapcraft" })),
    ).toMatch(/No saved account/);
    expect(backfillSourceReason(person, create(SourceConfigSchema, { ...source, enabled: false }))).toMatch(/disabled/);
  });

  it("requires a Jira account ID and Cloud mode", () => {
    const source = create(SourceConfigSchema, { enabled: true, sourceType: Platform.JIRA });
    expect(backfillSourceReason(person, source)).toBeUndefined();
    expect(
      backfillSourceReason(
        create(PersonSchema, {
          ...person,
          identities: [create(PlatformIdentitySchema, { platform: Platform.JIRA, username: "Alex" })],
        }),
        source,
      ),
    ).toMatch(/account ID/);
    expect(
      backfillSourceReason(person, create(SourceConfigSchema, { ...source, settings: { api_mode: "server" } })),
    ).toMatch(/Cloud/);
  });

  it("rejects inactive people and ambiguous saved accounts", () => {
    const source = create(SourceConfigSchema, { enabled: true, sourceType: Platform.JIRA });
    expect(backfillSourceReason(create(PersonSchema, { ...person, active: false }), source)).toMatch(/inactive/);
    expect(
      backfillSourceReason(
        create(PersonSchema, { ...person, identities: [...person.identities, person.identities[1]!] }),
        source,
      ),
    ).toMatch(/one saved account/);
  });

  it("validates actual calendar dates and pending/cancelling status", () => {
    expect(backfillDateError("2024-02-29")).toBeUndefined();
    expect(backfillDateError("2025-02-29")).toMatch(/valid/);
    expect(backfillDateError("2025-2-01")).toMatch(/format/);
    expect(backfillDateError("2999-01-01")).toMatch(/future/);
    expect(isActivePipeline("pending")).toBe(true);
    expect(isActivePipeline("cancelling")).toBe(true);
    expect(isActivePipeline("cancelled")).toBe(false);
  });

  it("uses the UTC eligibility date across local midnight", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-01T22:30:00Z"));
    vi.stubEnv("TZ", "Europe/Copenhagen");
    try {
      expect(todayDate()).toBe("2026-10-01");
      expect(backfillDateError("2026-10-02")).toMatch(/future/);
    } finally {
      vi.useRealTimers();
      vi.unstubAllEnvs();
    }
  });
});
