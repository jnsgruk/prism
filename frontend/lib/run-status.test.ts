import { describe, expect, it } from "vitest";

import { defaultStatus, statusConfig } from "./run-status";

describe("statusConfig", () => {
  it("has completed status with correct label and variant", () => {
    const s = statusConfig["completed"]!;
    expect(s.label).toBe("Completed");
    expect(s.variant).toBe("secondary");
  });

  it("has completed_with_warnings status", () => {
    const s = statusConfig["completed_with_warnings"]!;
    expect(s.label).toBe("Partial");
    expect(s.variant).toBe("outline");
  });

  it("has failed status with destructive variant", () => {
    const s = statusConfig["failed"]!;
    expect(s.label).toBe("Failed");
    expect(s.variant).toBe("destructive");
  });

  it("has cancelled status", () => {
    const s = statusConfig["cancelled"]!;
    expect(s.label).toBe("Cancelled");
    expect(s.variant).toBe("secondary");
  });

  it("has running status matching defaultStatus", () => {
    const s = statusConfig["running"]!;
    expect(s).toBe(defaultStatus);
    expect(s.label).toBe("Running");
    expect(s.variant).toBe("default");
  });

  it("each status has an icon", () => {
    for (const key of Object.keys(statusConfig)) {
      expect(statusConfig[key]!.icon).toBeTruthy();
    }
  });
});
