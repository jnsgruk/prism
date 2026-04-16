import { describe, expect, it } from "vite-plus/test";

import { normaliseProgress, parseProgress } from "./progress";
import type { RunProgress } from "./progress";

describe("parseProgress", () => {
  it("parses valid JSON", () => {
    const result = parseProgress('{"phase":"team_repos","repos_total":5}');
    expect(result).toEqual({ phase: "team_repos", repos_total: 5 });
  });

  it("returns null for undefined", () => {
    expect(parseProgress(undefined)).toBeNull();
  });

  it("returns null for empty string", () => {
    expect(parseProgress("")).toBeNull();
  });

  it("returns null for invalid JSON", () => {
    expect(parseProgress("{invalid}")).toBeNull();
  });
});

describe("normaliseProgress", () => {
  it('returns "Starting" for null progress', () => {
    expect(normaliseProgress("github", null)).toEqual({
      percent: null,
      label: "Starting",
    });
  });

  it("handles GitHub team_repos phase with totals (weighted 0-90%)", () => {
    const progress: RunProgress = { phase: "team_repos", repos_total: 10, repos_completed: 3 };
    expect(normaliseProgress("github", progress)).toEqual({
      percent: 27,
      label: "3/10 repos",
    });
  });

  it("handles GitHub team_repos phase with zero total", () => {
    const progress: RunProgress = { phase: "team_repos", repos_total: 0 };
    expect(normaliseProgress("github", progress)).toEqual({
      percent: null,
      label: "Fetching repos",
    });
  });

  it("handles GitHub member_search phase (weighted 90-100%)", () => {
    const progress: RunProgress = {
      phase: "member_search",
      search_users_total: 8,
      search_users_completed: 4,
    };
    expect(normaliseProgress("github", progress)).toEqual({
      percent: 95,
      label: "4/8 members",
    });
  });

  it("handles GitHub member_search with zero total", () => {
    const progress: RunProgress = { phase: "member_search" };
    expect(normaliseProgress("github", progress)).toEqual({
      percent: null,
      label: "Searching members",
    });
  });

  it("handles GitHub complete phase", () => {
    const progress: RunProgress = { phase: "complete" };
    expect(normaliseProgress("github", progress)).toEqual({
      percent: 100,
      label: "Finalising",
    });
  });

  it("handles GitHub unknown phase with status_message", () => {
    const progress: RunProgress = { phase: "unknown", status_message: "Warming up" };
    expect(normaliseProgress("github", progress)).toEqual({
      percent: null,
      label: "Warming up",
    });
  });

  it("handles GitHub unknown phase without status_message", () => {
    const progress: RunProgress = { phase: "unknown" };
    expect(normaliseProgress("github", progress)).toEqual({
      percent: null,
      label: "Starting",
    });
  });

  it("handles generic handler with status_message", () => {
    const progress: RunProgress = { status_message: "Processing page 3" };
    expect(normaliseProgress("jira", progress)).toEqual({
      percent: null,
      label: "Processing page 3",
    });
  });

  it('returns "Collecting" for generic handler without status_message', () => {
    expect(normaliseProgress("discourse", {})).toEqual({
      percent: null,
      label: "Collecting",
    });
  });

  it("shows pause note when rate_limit_reset_at is in the future", () => {
    const future = new Date(Date.now() + 15 * 60_000).toISOString();
    const progress: RunProgress = {
      phase: "team_repos",
      repos_total: 10,
      repos_completed: 5,
      rate_limit_reset_at: future,
    };
    const result = normaliseProgress("github", progress);
    expect(result.pauseNote).toMatch(/^Paused — resumes in \d+m$/);
  });

  it("omits pause note when rate_limit_reset_at is in the past", () => {
    const past = new Date(Date.now() - 60_000).toISOString();
    const progress: RunProgress = {
      phase: "team_repos",
      repos_total: 10,
      repos_completed: 5,
      rate_limit_reset_at: past,
    };
    const result = normaliseProgress("github", progress);
    expect(result.pauseNote).toBeUndefined();
  });

  it("omits pause note when rate_limit_reset_at is absent", () => {
    const progress: RunProgress = { phase: "team_repos", repos_total: 10, repos_completed: 3 };
    const result = normaliseProgress("github", progress);
    expect(result.pauseNote).toBeUndefined();
  });
});
