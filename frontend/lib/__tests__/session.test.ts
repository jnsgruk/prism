import { describe, it, expect, beforeEach } from "vitest";

import { getSessionToken, setSessionToken, clearSessionToken } from "@ps/session";

describe("session", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("returns null when no token is set", () => {
    expect(getSessionToken()).toBeNull();
  });

  it("stores and retrieves a token", () => {
    setSessionToken("test-token-123");
    expect(getSessionToken()).toBe("test-token-123");
  });

  it("clears the stored token", () => {
    setSessionToken("test-token-123");
    clearSessionToken();
    expect(getSessionToken()).toBeNull();
  });
});
