import type { ResolvedWorkspaceFile } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

type PendingFile = {
  path: string;
  resolve: (file: ResolvedWorkspaceFile) => void;
  reject: (error: unknown) => void;
};

type ResolveFiles = (request: {
  conversationId: string;
  paths: string[];
}) => Promise<{ files: ResolvedWorkspaceFile[] }>;

/** Combine checks mounted together while React Query retains its per-file cache. */
export const createWorkspaceFileResolver = (
  resolveFiles: ResolveFiles,
): ((conversationId: string, path: string) => Promise<ResolvedWorkspaceFile>) => {
  const pending = new Map<string, PendingFile[]>();

  const resolveBatch = async (conversationId: string, batch: PendingFile[]): Promise<void> => {
    try {
      const paths = [...new Set(batch.map((entry) => entry.path))];
      const response = await resolveFiles({ conversationId, paths });
      const files = new Map(response.files.map((file) => [file.path, file]));
      for (const entry of batch) {
        const file = files.get(entry.path);
        if (file) entry.resolve(file);
        else entry.reject(new Error("Could not verify file"));
      }
    } catch (error) {
      for (const entry of batch) entry.reject(error);
    }
  };

  return (conversationId, path) =>
    new Promise((resolve, reject) => {
      const entry = { path, resolve, reject };
      const batch = pending.get(conversationId);
      if (batch) {
        batch.push(entry);
        return;
      }

      const queued = [entry];
      pending.set(conversationId, queued);
      queueMicrotask(() => {
        pending.delete(conversationId);
        // The RPC accepts at most 128 paths per request.
        for (let start = 0; start < queued.length; start += 128) {
          void resolveBatch(conversationId, queued.slice(start, start + 128));
        }
      });
    });
};
