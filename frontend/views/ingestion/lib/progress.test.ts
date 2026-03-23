import { describe, expect, it } from "vitest";

import { extractDetail, normaliseProgress, parseProgress } from "./progress";
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

  it("handles GitHub team_repos phase with totals", () => {
    const progress: RunProgress = { phase: "team_repos", repos_total: 10, repos_completed: 3 };
    expect(normaliseProgress("github", progress)).toEqual({
      percent: 30,
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

  it("handles GitHub member_search phase", () => {
    const progress: RunProgress = {
      phase: "member_search",
      search_users_total: 8,
      search_users_completed: 4,
    };
    expect(normaliseProgress("github", progress)).toEqual({
      percent: 50,
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
});

describe("extractDetail", () => {
  it("returns null for null progress", () => {
    expect(extractDetail(null)).toBeNull();
  });

  it("returns null when no detail fields are present", () => {
    expect(extractDetail({})).toBeNull();
  });

  it("returns null when numeric fields are zero", () => {
    expect(extractDetail({ prs_fetched: 0, reviews_fetched: 0 })).toBeNull();
  });

  it("extracts PR and review counts", () => {
    const result = extractDetail({ prs_fetched: 42, reviews_fetched: 100 });
    expect(result).toEqual({
      prsFetched: 42,
      reviewsFetched: 100,
    });
  });

  it("extracts identities skipped", () => {
    const result = extractDetail({ prs_fetched: 1, identities_skipped: 5 });
    expect(result?.identitiesSkipped).toBe(5);
  });

  it("extracts rate limit info", () => {
    const result = extractDetail({
      prs_fetched: 1,
      rate_limit_remaining: 450,
      rate_limit_limit: 5000,
    });
    expect(result?.rateLimit).toEqual({ remaining: 450, limit: 5000 });
  });

  it("defaults rate_limit_remaining to 0 when missing", () => {
    const result = extractDetail({ prs_fetched: 1, rate_limit_limit: 5000 });
    expect(result?.rateLimit).toEqual({ remaining: 0, limit: 5000 });
  });

  it("includes statusMessage only when other stats exist", () => {
    // statusMessage alone doesn't produce a detail
    expect(extractDetail({ status_message: "hello" })).toBeNull();

    // statusMessage with stats is included
    const result = extractDetail({ prs_fetched: 10, status_message: "Fetching PRs" });
    expect(result?.statusMessage).toBe("Fetching PRs");
  });
});
