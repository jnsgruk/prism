import { describe, expect, it } from "vitest";

import { InsightPeriod } from "@ps/api/gen/canonical/prism/v1/common_pb";

import { periodKeyToInsightsPeriod } from "./use-insights";

describe("periodKeyToInsightsPeriod", () => {
  it('maps "1w" to LAST_WEEK', () => {
    expect(periodKeyToInsightsPeriod("1w")).toBe(InsightPeriod.LAST_WEEK);
  });

  it('maps "2w" to LAST_WEEK', () => {
    expect(periodKeyToInsightsPeriod("2w")).toBe(InsightPeriod.LAST_WEEK);
  });

  it('maps "1m" to LAST_MONTH', () => {
    expect(periodKeyToInsightsPeriod("1m")).toBe(InsightPeriod.LAST_MONTH);
  });

  it('maps "1q" to LAST_QUARTER', () => {
    expect(periodKeyToInsightsPeriod("1q")).toBe(InsightPeriod.LAST_QUARTER);
  });

  it('maps "1y" to LAST_YEAR', () => {
    expect(periodKeyToInsightsPeriod("1y")).toBe(InsightPeriod.LAST_YEAR);
  });

  it('maps "all" to LAST_YEAR', () => {
    expect(periodKeyToInsightsPeriod("all")).toBe(InsightPeriod.LAST_YEAR);
  });

  it("defaults to LAST_MONTH for unknown keys", () => {
    expect(periodKeyToInsightsPeriod("unknown")).toBe(InsightPeriod.LAST_MONTH);
    expect(periodKeyToInsightsPeriod("")).toBe(InsightPeriod.LAST_MONTH);
  });
});
