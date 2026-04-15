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

  it("includes rateLimitNote when rate limit data is present", () => {
    const progress: RunProgress = {
      phase: "team_repos",
      repos_total: 10,
      repos_completed: 5,
      rate_limit_remaining: 4334,
      rate_limit_limit: 5000,
    };
    const result = normaliseProgress("github", progress);
    expect(result.rateLimitNote).toBe("87% API calls left");
    expect(result.rateLimitLow).toBe(false);
  });

  it("sets rateLimitLow when remaining is below 10%", () => {
    const progress: RunProgress = {
      status_message: "Fetching issues",
      rate_limit_remaining: 30,
      rate_limit_limit: 350,
    };
    const result = normaliseProgress("jira", progress);
    expect(result.rateLimitNote).toBe("9% API calls left");
    expect(result.rateLimitLow).toBe(true);
  });

  it("omits rateLimitNote when rate limit data is absent", () => {
    const progress: RunProgress = { phase: "team_repos", repos_total: 10, repos_completed: 3 };
    const result = normaliseProgress("github", progress);
    expect(result.rateLimitNote).toBeUndefined();
    expect(result.rateLimitLow).toBeUndefined();
  });

  it("defaults rate_limit_remaining to 0 when missing", () => {
    const progress: RunProgress = {
      status_message: "Collecting",
      rate_limit_limit: 5000,
    };
    const result = normaliseProgress("jira", progress);
    expect(result.rateLimitNote).toBe("0% API calls left");
    expect(result.rateLimitLow).toBe(true);
  });
});
