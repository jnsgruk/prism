import { describe, it, expect, afterEach } from "vitest";

import { $currentUser, $isAuthenticated } from "@ps/stores";

describe("auth stores", () => {
  afterEach(() => {
    $currentUser.set(null);
    $isAuthenticated.set(false);
  });

  it("starts with no current user", () => {
    expect($currentUser.get()).toBeNull();
    expect($isAuthenticated.get()).toBe(false);
  });

  it("can set current user", () => {
    $currentUser.set({
      userId: "123",
      username: "admin",
      displayName: "Test Admin",
      role: "admin",
    });
    $isAuthenticated.set(true);

    expect($currentUser.get()?.username).toBe("admin");
    expect($isAuthenticated.get()).toBe(true);
  });
});
