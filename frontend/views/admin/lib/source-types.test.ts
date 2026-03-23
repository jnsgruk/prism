import { describe, expect, it } from "vitest";

import { baseSourceType, SECRET_KEYS_BY_TYPE, SOURCE_TYPES } from "./source-types";

describe("SOURCE_TYPES", () => {
  it("contains expected platforms", () => {
    const values = SOURCE_TYPES.map((s) => s.value);
    expect(values).toContain("github");
    expect(values).toContain("jira");
    expect(values).toContain("discourse");
  });
});

describe("SECRET_KEYS_BY_TYPE", () => {
  it("lists api_token for github", () => {
    expect(SECRET_KEYS_BY_TYPE.github).toEqual(["api_token"]);
  });

  it("lists api_token and email for jira", () => {
    expect(SECRET_KEYS_BY_TYPE.jira).toEqual(["api_token", "email"]);
  });

  it("lists empty array for mailing_list", () => {
    expect(SECRET_KEYS_BY_TYPE.mailing_list).toEqual([]);
  });
});

describe("baseSourceType", () => {
  it('normalises "discourse-ubuntu" to "discourse"', () => {
    expect(baseSourceType("discourse-ubuntu")).toBe("discourse");
  });

  it('normalises "discourse-" to "discourse"', () => {
    expect(baseSourceType("discourse-")).toBe("discourse");
  });

  it("returns github unchanged", () => {
    expect(baseSourceType("github")).toBe("github");
  });

  it("returns jira unchanged", () => {
    expect(baseSourceType("jira")).toBe("jira");
  });

  it('returns bare "discourse" unchanged (no dash)', () => {
    expect(baseSourceType("discourse")).toBe("discourse");
  });
});
