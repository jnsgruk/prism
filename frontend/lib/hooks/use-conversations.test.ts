import { create } from "@bufbuild/protobuf";
import { createRouterTransport } from "@connectrpc/connect";
import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vite-plus/test";

import {
  ConversationMessageSchema,
  ConversationSummarySchema,
  GetConversationResponseSchema,
  ListConversationsResponseSchema,
  ReasoningService,
} from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { TestWrapper } from "@ps/test-utils";

const mockConversations = [
  create(ConversationSummarySchema, {
    id: "conv-1",
    title: "Team velocity this sprint",
    status: "completed",
    modelName: "claude-sonnet",
    containerStatus: "running",
    totalToolCalls: 5,

    messageCount: 4,
  }),
  create(ConversationSummarySchema, {
    id: "conv-2",
    title: "PR review turnaround",
    status: "completed",
    modelName: "claude-sonnet",
    containerStatus: "stopped",
    totalToolCalls: 3,

    messageCount: 2,
  }),
];

const mockMessages = [
  create(ConversationMessageSchema, {
    id: "msg-1",
    role: "user",
    content: "What is the team velocity?",
    promptTokens: 50,
    completionTokens: 0,
  }),
  create(ConversationMessageSchema, {
    id: "msg-2",
    role: "assistant",
    content: "The team velocity is **42 points** this sprint.",
    promptTokens: 200,
    completionTokens: 150,
  }),
];

vi.mock("@ps/api/transport", () => ({
  transport: createRouterTransport(({ service }) => {
    service(ReasoningService, {
      listConversations: () =>
        create(ListConversationsResponseSchema, {
          conversations: mockConversations,
          totalCount: 2,
        }),
      getConversation: () =>
        create(GetConversationResponseSchema, {
          conversation: mockConversations[0],
          messages: mockMessages,
        }),
      // Stubs for remaining service methods
      getAiSettings: () => ({}),
      updateAiSettings: () => ({}),
      setProviderSecret: () => ({}),
      testProvider: () => ({}),
      getUsageSummary: () => ({}),
      listAiModels: () => ({}),
      refreshModelCatalogue: () => ({}),
      getEnrichments: () => ({}),
      getEnrichmentsByContributions: () => ({}),
      getEnrichmentPipelineStatus: () => ({}),
      deleteEnrichmentsByType: () => ({}),
      findSimilar: () => ({}),
      searchByText: () => ({}),
      getEmbeddingStatus: () => ({}),
      askQuestion: async function* (): AsyncGenerator<Record<string, never>> {},
      saveInsightFromConversation: () => ({}),
    });
  }),
}));

describe("conversation hooks", () => {
  describe("conversationKeys", () => {
    it("list() returns correct key shape", async () => {
      const { conversationKeys } = await import("./use-conversations");
      expect(conversationKeys.list()).toEqual(["conversations", "list"]);
    });

    it("detail(id) returns correct key shape", async () => {
      const { conversationKeys } = await import("./use-conversations");
      expect(conversationKeys.detail("conv-1")).toEqual(["conversations", "detail", "conv-1"]);
    });
  });

  describe("useListConversations", () => {
    it("returns conversations from mocked service", async () => {
      const { useListConversations } = await import("./use-conversations");
      const { result } = renderHook(() => useListConversations(1, 25), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.conversations).toHaveLength(2);
      expect(result.current.data?.conversations[0]?.id).toBe("conv-1");
      expect(result.current.data?.conversations[0]?.title).toBe("Team velocity this sprint");
      expect(result.current.data?.totalCount).toBe(2);
    });
  });

  describe("useGetConversation", () => {
    it("returns conversation detail from mock", async () => {
      const { useGetConversation } = await import("./use-conversations");
      const { result } = renderHook(() => useGetConversation("conv-1"), { wrapper: TestWrapper });

      await waitFor(() => expect(result.current.isSuccess).toBe(true));
      expect(result.current.data?.conversation?.id).toBe("conv-1");
      expect(result.current.data?.messages).toHaveLength(2);
      expect(result.current.data?.messages[0]?.role).toBe("user");
      expect(result.current.data?.messages[1]?.content).toContain("42 points");
    });

    it("is disabled when conversationId is empty", async () => {
      const { useGetConversation } = await import("./use-conversations");
      const { result } = renderHook(() => useGetConversation(""), { wrapper: TestWrapper });

      // Should never transition to loading or success since the query is disabled
      expect(result.current.fetchStatus).toBe("idle");
      expect(result.current.isSuccess).toBe(false);
    });
  });
});
