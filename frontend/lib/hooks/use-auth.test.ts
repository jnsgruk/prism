import { create } from "@bufbuild/protobuf";
import { createRouterTransport } from "@connectrpc/connect";
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  AuthService,
  CompleteSetupResponseSchema,
  GetCurrentUserResponseSchema,
  GetSetupStatusResponseSchema,
  LoginResponseSchema,
  LogoutResponseSchema,
} from "@ps/api/gen/canonical/prism/v1/auth_pb";
import { clearSessionToken, getSessionToken, setSessionToken } from "@ps/session";
import { TestWrapper } from "@ps/test-utils";

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(AuthService, {
      getSetupStatus: () => create(GetSetupStatusResponseSchema, { setupComplete: true }),
      getCurrentUser: () =>
        create(GetCurrentUserResponseSchema, {
          username: "admin",
          displayName: "Admin User",
          role: "admin",
        }),
      completeSetup: () => create(CompleteSetupResponseSchema, { sessionToken: "setup-token-123" }),
      login: () => create(LoginResponseSchema, { sessionToken: "login-token-456" }),
      logout: () => create(LogoutResponseSchema, {}),
    });
  }),
}));

describe("auth hooks", () => {
  beforeEach(() => {
    clearSessionToken();
  });

  afterEach(() => {
    clearSessionToken();
  });

  describe("useSetupStatus", () => {
    it("returns setup completion status", async () => {
      const { useSetupStatus } = await import("./use-auth");
      const { result } = renderHook(() => useSetupStatus(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data).toBe(true);
    });

    it("uses correct query key", async () => {
      const { authKeys } = await import("./use-auth");
      expect(authKeys.setupStatus()).toEqual(["auth", "setupStatus"]);
    });
  });

  describe("useCurrentUser", () => {
    it("returns current user data", async () => {
      const { useCurrentUser } = await import("./use-auth");
      const { result } = renderHook(() => useCurrentUser(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.username).toBe("admin");
      expect(result.current.data?.displayName).toBe("Admin User");
    });

    it("uses correct query key", async () => {
      const { authKeys } = await import("./use-auth");
      expect(authKeys.currentUser()).toEqual(["auth", "currentUser"]);
    });
  });

  describe("useLogin", () => {
    it("stores session token on success", async () => {
      const { useLogin } = await import("./use-auth");
      const { result } = renderHook(() => useLogin(), { wrapper: TestWrapper });

      result.current.mutate({ username: "admin", password: "pass" });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(getSessionToken()).toBe("login-token-456");
    });
  });

  describe("useCompleteSetup", () => {
    it("stores session token on success", async () => {
      const { useCompleteSetup } = await import("./use-auth");
      const { result } = renderHook(() => useCompleteSetup(), { wrapper: TestWrapper });

      result.current.mutate({
        username: "admin",
        displayName: "Admin",
        password: "secure123",
      });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(getSessionToken()).toBe("setup-token-123");
    });
  });

  describe("useLogout", () => {
    it("clears session token on settle", async () => {
      setSessionToken("existing-token");
      expect(getSessionToken()).toBe("existing-token");

      const { useLogout } = await import("./use-auth");
      const { result } = renderHook(() => useLogout(), { wrapper: TestWrapper });

      result.current.mutate();

      await waitFor(() => expect(getSessionToken()).toBeNull());
    });
  });

  describe("authKeys", () => {
    it("builds hierarchical query keys", async () => {
      const { authKeys } = await import("./use-auth");
      expect(authKeys.all).toEqual(["auth"]);
      expect(authKeys.setupStatus()).toEqual(["auth", "setupStatus"]);
      expect(authKeys.currentUser()).toEqual(["auth", "currentUser"]);
    });
  });
});
