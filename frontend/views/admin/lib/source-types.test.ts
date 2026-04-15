import { describe, expect, it } from "vite-plus/test";

import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";

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
    expect(SECRET_KEYS_BY_TYPE[baseSourceType(Platform.GITHUB)]).toEqual(["api_token"]);
  });

  it("lists api_token and email for jira", () => {
    expect(SECRET_KEYS_BY_TYPE[baseSourceType(Platform.JIRA)]).toEqual(["api_token", "email"]);
  });

  it("lists empty array for mailing_list", () => {
    expect(SECRET_KEYS_BY_TYPE[baseSourceType(Platform.MAILING_LIST)]).toEqual([]);
  });
});

describe("baseSourceType", () => {
  it("returns github for Platform.GITHUB", () => {
    expect(baseSourceType(Platform.GITHUB)).toBe("github");
  });

  it("returns jira for Platform.JIRA", () => {
    expect(baseSourceType(Platform.JIRA)).toBe("jira");
  });

  it("returns discourse for Platform.DISCOURSE", () => {
    expect(baseSourceType(Platform.DISCOURSE)).toBe("discourse");
  });

  it("returns google drive for Platform.GOOGLE_DRIVE", () => {
    expect(baseSourceType(Platform.GOOGLE_DRIVE)).toBe("google drive");
  });

  it("returns mailing list for Platform.MAILING_LIST", () => {
    expect(baseSourceType(Platform.MAILING_LIST)).toBe("mailing list");
  });
});
