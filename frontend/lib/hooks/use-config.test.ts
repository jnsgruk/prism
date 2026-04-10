import { create } from "@bufbuild/protobuf";
import { createRouterTransport } from "@connectrpc/connect";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import {
  ConfigService,
  CreateSourceResponseSchema,
  DeleteSourceResponseSchema,
  GetSourceResponseSchema,
  ListSourcesResponseSchema,
  SetSecretResponseSchema,
  SourceConfigSchema,
  TestConnectionResponseSchema,
  UpdateSourceResponseSchema,
} from "@ps/api/gen/canonical/prism/v1/config_pb";
import { TestWrapper } from "@ps/test-utils";

const mockSource = create(SourceConfigSchema, {
  id: "src-1",
  sourceType: Platform.GITHUB,
  name: "github-main",
  enabled: true,
});

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(ConfigService, {
      listSources: () => create(ListSourcesResponseSchema, { sources: [mockSource] }),
      getSource: () => create(GetSourceResponseSchema, { source: mockSource }),
      createSource: () => create(CreateSourceResponseSchema, { source: mockSource }),
      updateSource: () => create(UpdateSourceResponseSchema, {}),
      deleteSource: () => create(DeleteSourceResponseSchema, {}),
      setSecret: () => create(SetSecretResponseSchema, {}),
      testConnection: () => create(TestConnectionResponseSchema, { success: true }),
    });
  }),
}));

describe("config hooks", () => {
  describe("useListSources", () => {
    it("fetches and returns sources array", async () => {
      const { useListSources } = await import("./use-config");
      const { result } = renderHook(() => useListSources(), { wrapper: TestWrapper });

      await waitFor(() => {
        expect(result.current.isSuccess).toBe(true);
        expect(result.current.data).toHaveLength(1);
        expect(result.current.data?.[0]?.name).toBe("github-main");
      });
    });
  });

  describe("useGetSource", () => {
    it("fetches a single source by ID", async () => {
      const { useGetSource } = await import("./use-config");
      const { result } = renderHook(() => useGetSource("src-1"), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.sourceType).toBe(Platform.GITHUB);
    });

    it("is disabled when sourceId is empty", async () => {
      const { useGetSource } = await import("./use-config");
      const { result } = renderHook(() => useGetSource(""), { wrapper: TestWrapper });

      // Should not fetch — query stays in idle/pending state
      expect(result.current.fetchStatus).toBe("idle");
    });
  });

  describe("useCreateSource", () => {
    it("creates a source and succeeds", async () => {
      const { useCreateSource } = await import("./use-config");
      const { result } = renderHook(() => useCreateSource(), { wrapper: TestWrapper });

      result.current.mutate({
        sourceType: Platform.GITHUB,
        name: "new-source",
      });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useUpdateSource", () => {
    it("updates a source and succeeds", async () => {
      const { useUpdateSource } = await import("./use-config");
      const { result } = renderHook(() => useUpdateSource(), { wrapper: TestWrapper });

      result.current.mutate({ sourceId: "src-1", enabled: false });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useDeleteSource", () => {
    it("deletes a source and succeeds", async () => {
      const { useDeleteSource } = await import("./use-config");
      const { result } = renderHook(() => useDeleteSource(), { wrapper: TestWrapper });

      result.current.mutate("src-1");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useSetSecret", () => {
    it("sets a secret and succeeds", async () => {
      const { useSetSecret } = await import("./use-config");
      const { result } = renderHook(() => useSetSecret(), { wrapper: TestWrapper });

      result.current.mutate({
        sourceId: "src-1",
        secretKey: "api_token",
        secretValue: "ghp_xxx",
      });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useTestConnection", () => {
    it("tests connection and returns result", async () => {
      const { useTestConnection } = await import("./use-config");
      const { result } = renderHook(() => useTestConnection(), { wrapper: TestWrapper });

      result.current.mutate("src-1");

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.success).toBe(true);
    });
  });

  describe("configKeys", () => {
    it("builds hierarchical query keys", async () => {
      const { configKeys } = await import("./use-config");
      expect(configKeys.all).toEqual(["config"]);
      expect(configKeys.sources()).toEqual(["config", "sources"]);
      expect(configKeys.source("abc")).toEqual(["config", "source", "abc"]);
    });
  });
});
