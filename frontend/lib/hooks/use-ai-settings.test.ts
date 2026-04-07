import { create } from "@bufbuild/protobuf";
import { createRouterTransport } from "@connectrpc/connect";
import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { AiProvider } from "@ps/api/gen/canonical/prism/v1/common_pb";
import {
  GetAiSettingsResponseSchema,
  GetStorageHealthResponseSchema,
  ListAiModelsResponseSchema,
  ReasoningService,
  RefreshModelCatalogueResponseSchema,
  SetProviderSecretResponseSchema,
  TestProviderResponseSchema,
  UpdateAiSettingsResponseSchema,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { TestWrapper } from "@ps/test-utils";

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(ReasoningService, {
      getAiSettings: () =>
        create(GetAiSettingsResponseSchema, {
          settings: { enrichment: { provider: AiProvider.GOOGLE, model: "test-model" } },
        }),
      updateAiSettings: () => create(UpdateAiSettingsResponseSchema, {}),
      setProviderSecret: () => create(SetProviderSecretResponseSchema, {}),
      testProvider: () => create(TestProviderResponseSchema, { success: true }),
      getStorageHealth: () => create(GetStorageHealthResponseSchema, { healthy: true }),
      listAiModels: () => create(ListAiModelsResponseSchema, { models: [] }),
      refreshModelCatalogue: () => create(RefreshModelCatalogueResponseSchema, {}),
      // Provide stubs for any other methods the service might expect
      getEnrichmentPipelineStatus: () => ({}),
      getUsageSummary: () => ({}),
    });
  }),
}));

describe("AI settings hooks", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("useAiSettings", () => {
    it("fetches AI settings", async () => {
      vi.useRealTimers();
      const { useAiSettings } = await import("./use-ai-settings");
      const { result } = renderHook(() => useAiSettings(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.enrichment?.provider).toBe(AiProvider.GOOGLE);
    });
  });

  describe("useUpdateAiSettings", () => {
    it("updates settings and succeeds", async () => {
      vi.useRealTimers();
      const { useUpdateAiSettings } = await import("./use-ai-settings");
      const { result } = renderHook(() => useUpdateAiSettings(), { wrapper: TestWrapper });

      result.current.mutate({ enrichment: { provider: AiProvider.GOOGLE, model: "gemini-flash" } });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useSetProviderSecret", () => {
    it("sets provider secret and succeeds", async () => {
      vi.useRealTimers();
      const { useSetProviderSecret } = await import("./use-ai-settings");
      const { result } = renderHook(() => useSetProviderSecret(), { wrapper: TestWrapper });

      result.current.mutate({ provider: AiProvider.GOOGLE, secretValue: "sk-xxx" });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("useTestProvider", () => {
    it("tests provider connectivity", async () => {
      vi.useRealTimers();
      const { useTestProvider } = await import("./use-ai-settings");
      const { result } = renderHook(() => useTestProvider(), { wrapper: TestWrapper });

      result.current.mutate({ provider: AiProvider.GOOGLE });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.success).toBe(true);
    });
  });

  describe("useStorageHealth", () => {
    it("fetches storage health status", async () => {
      vi.useRealTimers();
      const { useStorageHealth } = await import("./use-ai-settings");
      const { result } = renderHook(() => useStorageHealth(), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.healthy).toBe(true);
    });
  });

  describe("useAiModels", () => {
    it("fetches models for a provider and capability", async () => {
      vi.useRealTimers();
      const { useAiModels } = await import("./use-ai-settings");
      const { result } = renderHook(() => useAiModels(AiProvider.GOOGLE, "chat"), {
        wrapper: TestWrapper,
      });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.models).toEqual([]);
    });
  });

  describe("useRefreshModelCatalogue", () => {
    it("triggers catalogue refresh and succeeds", async () => {
      vi.useRealTimers();
      const { useRefreshModelCatalogue } = await import("./use-ai-settings");
      const { result } = renderHook(() => useRefreshModelCatalogue(), {
        wrapper: TestWrapper,
      });

      result.current.mutate();

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
    });
  });

  describe("aiKeys", () => {
    it("builds hierarchical query keys", async () => {
      const { aiKeys } = await import("./use-ai-settings");
      expect(aiKeys.all).toEqual(["ai"]);
      expect(aiKeys.settings()).toEqual(["ai", "settings"]);
      expect(aiKeys.models("google", "chat")).toEqual(["ai", "models", "google", "chat"]);
      expect(aiKeys.storageHealth()).toEqual(["ai", "storage-health"]);
    });
  });
});
