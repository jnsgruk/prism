import { create } from "@bufbuild/protobuf";
import { describe, expect, it, vi } from "vite-plus/test";

import { ResolvedWorkspaceFileSchema, type ResolvedWorkspaceFile } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

import { createWorkspaceFileResolver } from "./workspace-file-resolution";

type ResolveFiles = (request: {
  conversationId: string;
  paths: string[];
}) => Promise<{ files: ResolvedWorkspaceFile[] }>;

describe("workspace verification batches", () => {
  it("keeps conversations separate even when filenames match", async () => {
    const request = vi.fn<ResolveFiles>(async ({ conversationId, paths }) => ({
      files: paths.map((path) => create(ResolvedWorkspaceFileSchema, { path, contentType: conversationId })),
    }));
    const resolve = createWorkspaceFileResolver(request);
    const files = await Promise.all([resolve("first", "report.pdf"), resolve("second", "report.pdf")]);
    expect(request).toHaveBeenCalledTimes(2);
    expect(files.map((file) => file.contentType)).toEqual(["first", "second"]);
  });

  it("fails omitted paths without hiding successfully verified neighbors", async () => {
    const request = vi.fn<ResolveFiles>(async () => ({
      files: [create(ResolvedWorkspaceFileSchema, { path: "available.pdf" })],
    }));
    const resolve = createWorkspaceFileResolver(request);
    const files = await Promise.allSettled([
      resolve("conversation", "available.pdf"),
      resolve("conversation", "omitted.pdf"),
    ]);
    expect(files[0].status).toBe("fulfilled");
    expect(files[1]).toEqual({ status: "rejected", reason: new Error("Could not verify file") });
    expect(request).toHaveBeenCalledTimes(1);
  });

  it("propagates a batch error to all files and permits a fresh retry", async () => {
    const error = new Error("Storage unavailable");
    const request = vi
      .fn<ResolveFiles>(async ({ paths }) => ({
        files: paths.map((path) => create(ResolvedWorkspaceFileSchema, { path })),
      }))
      .mockRejectedValueOnce(error);
    const resolve = createWorkspaceFileResolver(request);
    const files = await Promise.allSettled([
      resolve("conversation", "first.pdf"),
      resolve("conversation", "second.pdf"),
    ]);
    expect(files).toEqual([
      { status: "rejected", reason: error },
      { status: "rejected", reason: error },
    ]);
    await expect(resolve("conversation", "first.pdf")).resolves.toMatchObject({ path: "first.pdf" });
    expect(request).toHaveBeenCalledTimes(2);
  });
});
