import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  formatDateOnly,
  formatDuration,
  formatFullTimestamp,
  formatRelativeTime,
  formatRelativeTimeIso,
  formatTimestamp,
} from "./format";

const ts = (seconds: number): { seconds: bigint } => ({ seconds: BigInt(seconds) });

// 2026-03-16 14:30:00 UTC
const FIXED_NOW = 1742135400000;

describe("formatTimestamp", () => {
  it("returns em-dash for undefined", () => {
    expect(formatTimestamp(undefined)).toBe("—");
  });

  it("formats a valid timestamp with 24h time", () => {
    const result = formatTimestamp(ts(1742135400));
    // Locale-dependent but should contain month + 24h time
    expect(result).toMatch(/\d{2}:\d{2}/);
    expect(result).not.toMatch(/AM|PM/);
  });

  it("handles epoch zero", () => {
    const result = formatTimestamp(ts(0));
    expect(result).toBeTruthy();
    expect(result).not.toBe("—");
  });
});

describe("formatFullTimestamp", () => {
  it("returns em-dash for undefined", () => {
    expect(formatFullTimestamp(undefined)).toBe("—");
  });

  it("returns a full locale string", () => {
    const result = formatFullTimestamp(ts(1742135400));
    expect(result).toBeTruthy();
    expect(result).not.toBe("—");
  });
});

describe("formatDateOnly", () => {
  it('returns "Never" for undefined', () => {
    expect(formatDateOnly(undefined)).toBe("Never");
  });

  it("formats date with year, month, and day", () => {
    const result = formatDateOnly(ts(1742135400));
    // Should contain year
    expect(result).toMatch(/202[0-9]/);
  });
});

describe("formatDuration", () => {
  it("returns em-dash when start is missing", () => {
    expect(formatDuration(undefined, ts(100))).toBe("—");
  });

  it("returns em-dash when end is missing", () => {
    expect(formatDuration(ts(100), undefined)).toBe("—");
  });

  it("returns em-dash when both are missing", () => {
    expect(formatDuration(undefined, undefined)).toBe("—");
  });

  it("formats seconds only for short durations", () => {
    expect(formatDuration(ts(100), ts(108))).toBe("8s");
  });

  it("formats minutes and seconds", () => {
    expect(formatDuration(ts(100), ts(235))).toBe("2m 15s");
  });

  it("handles zero duration", () => {
    expect(formatDuration(ts(100), ts(100))).toBe("0s");
  });
});

describe("formatRelativeTime", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(FIXED_NOW);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns "Never" for undefined', () => {
    expect(formatRelativeTime(undefined)).toBe("Never");
  });

  it('returns "just now" for very recent timestamps', () => {
    const nowSec = Math.floor(FIXED_NOW / 1000);
    expect(formatRelativeTime(ts(nowSec))).toBe("just now");
  });

  it("returns minutes ago", () => {
    const fiveMinAgo = Math.floor(FIXED_NOW / 1000) - 300;
    expect(formatRelativeTime(ts(fiveMinAgo))).toBe("5m ago");
  });

  it("returns hours ago", () => {
    const twoHoursAgo = Math.floor(FIXED_NOW / 1000) - 7200;
    expect(formatRelativeTime(ts(twoHoursAgo))).toBe("2h ago");
  });

  it("returns days ago", () => {
    const threeDaysAgo = Math.floor(FIXED_NOW / 1000) - 259200;
    expect(formatRelativeTime(ts(threeDaysAgo))).toBe("3d ago");
  });

  it("falls back to date after 30 days", () => {
    const thirtyOneDaysAgo = Math.floor(FIXED_NOW / 1000) - 31 * 86400;
    const result = formatRelativeTime(ts(thirtyOneDaysAgo));
    expect(result).not.toMatch(/ago$/);
    // Should be a formatted date
    expect(result).toMatch(/202[0-9]/);
  });
});

describe("formatRelativeTimeIso", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(FIXED_NOW);
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns "just now" for current time', () => {
    expect(formatRelativeTimeIso(new Date(FIXED_NOW).toISOString())).toBe("just now");
  });

  it("returns minutes ago", () => {
    const fiveMinAgo = new Date(FIXED_NOW - 300_000).toISOString();
    expect(formatRelativeTimeIso(fiveMinAgo)).toBe("5m ago");
  });

  it("returns hours ago", () => {
    const twoHoursAgo = new Date(FIXED_NOW - 7_200_000).toISOString();
    expect(formatRelativeTimeIso(twoHoursAgo)).toBe("2h ago");
  });

  it("returns days ago for older timestamps", () => {
    const threeDaysAgo = new Date(FIXED_NOW - 259_200_000).toISOString();
    expect(formatRelativeTimeIso(threeDaysAgo)).toBe("3d ago");
  });
});
