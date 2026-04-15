import { describe, expect, it } from "vite-plus/test";

import { fmtFloat, fmtHours, fmtPercent, instanceLabel } from "./format-metrics";

describe("fmtHours", () => {
  it("formats positive hours", () => {
    expect(fmtHours(2.5)).toBe("2.5h");
  });

  it("returns em-dash for zero", () => {
    expect(fmtHours(0)).toBe("—");
  });

  it("formats to one decimal place", () => {
    expect(fmtHours(1.456)).toBe("1.5h");
  });
});

describe("fmtFloat", () => {
  it("formats positive float to one decimal", () => {
    expect(fmtFloat(3.14)).toBe("3.1");
  });

  it("returns em-dash for zero", () => {
    expect(fmtFloat(0)).toBe("—");
  });

  it("rounds correctly", () => {
    expect(fmtFloat(2.95)).toBe("3.0");
  });
});

describe("fmtPercent", () => {
  it("formats ratio as percentage", () => {
    expect(fmtPercent(0.42)).toBe("42%");
  });

  it("rounds to nearest integer", () => {
    expect(fmtPercent(0.456)).toBe("46%");
  });

  it("handles zero", () => {
    expect(fmtPercent(0)).toBe("0%");
  });

  it("handles 100%", () => {
    expect(fmtPercent(1)).toBe("100%");
  });
});

describe("instanceLabel", () => {
  it('extracts and capitalises suffix from "discourse-ubuntu"', () => {
    expect(instanceLabel("discourse-ubuntu")).toBe("Ubuntu");
  });

  it("capitalises non-discourse strings", () => {
    expect(instanceLabel("github")).toBe("Github");
  });

  it('returns original for bare "discourse"', () => {
    expect(instanceLabel("discourse")).toBe("discourse");
  });

  it("capitalises multi-word suffixes", () => {
    expect(instanceLabel("discourse-my-forum")).toBe("My Forum");
  });

  it("handles discourse- with empty suffix", () => {
    // "discourse-" has empty suffix after replace, returns original
    expect(instanceLabel("discourse-")).toBe("discourse-");
  });
});
