import { describe, expect, it } from "vitest";

import { RunStatus } from "@ps/api/gen/canonical/prism/v1/common_pb";

import { defaultStatus, statusConfig } from "./run-status";

describe("statusConfig", () => {
  it("has completed status with correct label and variant", () => {
    const s = statusConfig[RunStatus.COMPLETED]!;
    expect(s.label).toBe("Completed");
    expect(s.variant).toBe("secondary");
  });

  it("has completed_with_warnings status", () => {
    const s = statusConfig[RunStatus.COMPLETED_WITH_WARNINGS]!;
    expect(s.label).toBe("Partial");
    expect(s.variant).toBe("outline");
  });

  it("has failed status with destructive variant", () => {
    const s = statusConfig[RunStatus.FAILED]!;
    expect(s.label).toBe("Failed");
    expect(s.variant).toBe("destructive");
  });

  it("has cancelled status", () => {
    const s = statusConfig[RunStatus.CANCELLED]!;
    expect(s.label).toBe("Cancelled");
    expect(s.variant).toBe("secondary");
  });

  it("has running status matching defaultStatus", () => {
    const s = statusConfig[RunStatus.RUNNING]!;
    expect(s).toBe(defaultStatus);
    expect(s.label).toBe("Running");
    expect(s.variant).toBe("default");
  });

  it("each status has an icon", () => {
    const numericValues = Object.values(RunStatus).filter((v): v is number => typeof v === "number");
    for (const key of numericValues) {
      expect(statusConfig[key as RunStatus]!.icon).toBeTruthy();
    }
  });
});
