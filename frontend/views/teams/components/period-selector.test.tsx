import { PeriodType } from "@ps/api/gen/canonical/prism/v1/metrics_pb";
import { describe, expect, it } from "vitest";

import { buildPeriod, defaultPeriodKey } from "./period-selector";

describe("buildPeriod", () => {
  it("returns WEEK type for 1w key", () => {
    const period = buildPeriod("1w");
    expect(period.type).toBe(PeriodType.WEEK);
  });

  it("returns WEEK type for 2w key", () => {
    const period = buildPeriod("2w");
    expect(period.type).toBe(PeriodType.WEEK);
  });

  it("returns MONTH type for 1m key", () => {
    const period = buildPeriod("1m");
    expect(period.type).toBe(PeriodType.MONTH);
  });

  it("returns QUARTER type for 1q key", () => {
    const period = buildPeriod("1q");
    expect(period.type).toBe(PeriodType.QUARTER);
  });

  it("returns QUARTER type for 1y key", () => {
    const period = buildPeriod("1y");
    expect(period.type).toBe(PeriodType.QUARTER);
  });

  it("returns QUARTER type for all key", () => {
    const period = buildPeriod("all");
    expect(period.type).toBe(PeriodType.QUARTER);
    expect(period.start).toBe("2000-01-01");
  });

  it("generates start date 7 days before end for 1w", () => {
    const period = buildPeriod("1w");
    const start = new Date(period.start);
    const end = new Date(period.end);
    const diffDays = Math.round((end.getTime() - start.getTime()) / (1000 * 60 * 60 * 24));
    expect(diffDays).toBe(7);
  });

  it("generates start date 14 days before end for 2w", () => {
    const period = buildPeriod("2w");
    const start = new Date(period.start);
    const end = new Date(period.end);
    const diffDays = Math.round((end.getTime() - start.getTime()) / (1000 * 60 * 60 * 24));
    expect(diffDays).toBe(14);
  });

  it("falls back to 1m when unknown key is provided", () => {
    const period = buildPeriod("unknown");
    expect(period.type).toBe(PeriodType.MONTH);
  });

  it("returns ISO 8601 date format for start and end", () => {
    const period = buildPeriod("1m");
    expect(period.start).toMatch(/^\d{4}-\d{2}-\d{2}$/);
    expect(period.end).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});

describe("defaultPeriodKey", () => {
  it("is 1m", () => {
    expect(defaultPeriodKey).toBe("1m");
  });
});
