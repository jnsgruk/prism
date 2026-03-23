import { describe, expect, it } from "vitest";

import { periodKeyToInsightsPeriod } from "./use-insights";

describe("periodKeyToInsightsPeriod", () => {
  it('maps "1w" to "last_week"', () => {
    expect(periodKeyToInsightsPeriod("1w")).toBe("last_week");
  });

  it('maps "2w" to "last_week"', () => {
    expect(periodKeyToInsightsPeriod("2w")).toBe("last_week");
  });

  it('maps "1m" to "last_month"', () => {
    expect(periodKeyToInsightsPeriod("1m")).toBe("last_month");
  });

  it('maps "1q" to "last_quarter"', () => {
    expect(periodKeyToInsightsPeriod("1q")).toBe("last_quarter");
  });

  it('maps "1y" to "last_year"', () => {
    expect(periodKeyToInsightsPeriod("1y")).toBe("last_year");
  });

  it('maps "all" to "last_year"', () => {
    expect(periodKeyToInsightsPeriod("all")).toBe("last_year");
  });

  it('defaults to "last_month" for unknown keys', () => {
    expect(periodKeyToInsightsPeriod("unknown")).toBe("last_month");
    expect(periodKeyToInsightsPeriod("")).toBe("last_month");
  });
});
