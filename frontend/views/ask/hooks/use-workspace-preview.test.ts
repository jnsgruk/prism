import { useWorkspacePreview } from "@/views/ask/hooks/use-workspace-preview";
import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { download } = vi.hoisted(() => ({ download: vi.fn<() => Promise<{ blobUrl: string; contentType: string }>>() }));
vi.mock("@/lib/hooks/use-conversations", () => ({
  useDownloadWorkspaceFile: (): { mutateAsync: typeof download } => ({ mutateAsync: download }),
}));
vi.mock("sonner", () => ({ toast: { error: vi.fn<(message: string) => void>() } }));

const deferred = <T>(): { promise: Promise<T>; resolve: (value: T) => void } => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => {
    resolve = yes;
  });
  return { promise, resolve };
};
const artifact = (id: string): { id: string; displayName: string; sizeBytes: number } => ({
  id,
  displayName: id,
  sizeBytes: 123,
});
const file = (blobUrl: string, contentType = "application/pdf"): { blobUrl: string; contentType: string } => ({
  blobUrl,
  contentType,
});

beforeEach(() => {
  download.mockReset();
  vi.spyOn(URL, "revokeObjectURL").mockImplementation(() => {});
});
afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("workspace preview ownership", () => {
  it("discards out-of-order downloads and revokes owned URLs exactly once", async () => {
    const a = deferred<ReturnType<typeof file>>();
    const b = deferred<ReturnType<typeof file>>();
    download.mockReturnValueOnce(a.promise).mockReturnValueOnce(b.promise);
    const { result, unmount } = renderHook(() => useWorkspacePreview("conversation", true));
    let pendingA!: Promise<void>;
    let pendingB!: Promise<void>;
    act(() => {
      pendingA = result.current.select(artifact("a"));
    });
    act(() => {
      pendingB = result.current.select(artifact("b"));
    });
    await act(async () => {
      b.resolve(file("blob:b"));
      await pendingB;
    });
    await act(async () => {
      a.resolve(file("blob:a"));
      await pendingA;
    });
    expect(result.current.state?.url).toBe("blob:b");
    expect(result.current.selectedPath).toBe("b");
    expect(URL.revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:a");
    unmount();
    expect(URL.revokeObjectURL).toHaveBeenCalledWith("blob:b");
    expect(URL.revokeObjectURL).toHaveBeenCalledTimes(2);
  });

  it.each(["close", "conversation", "hide"])("invalidates loading on %s", async (action) => {
    const pending = deferred<ReturnType<typeof file>>();
    download.mockReturnValue(pending.promise);
    const { result, rerender } = renderHook(({ conversationId, open }) => useWorkspacePreview(conversationId, open), {
      initialProps: { conversationId: "a", open: true },
    });
    let selection!: Promise<void>;
    act(() => {
      selection = result.current.select(artifact("a.pdf"));
    });
    if (action === "close") act(() => result.current.close());
    if (action === "conversation") rerender({ conversationId: "b", open: true });
    if (action === "hide") rerender({ conversationId: "a", open: false });
    await act(async () => {
      pending.resolve(file("blob:stale"));
      await selection;
    });
    expect(URL.revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:stale");
    expect(result.current.state).toBeNull();
    expect(result.current.isLoading).toBe(false);
    expect(result.current.selectedPath).toBeNull();
    expect(result.current.dialogOpen).toBe(false);
  });

  it("releases a download resolved in the same batch as unmount before state commits", async () => {
    const pending = deferred<ReturnType<typeof file>>();
    download.mockReturnValue(pending.promise);
    const { result, unmount } = renderHook(() => useWorkspacePreview("a", true));
    let selection!: Promise<void>;
    act(() => {
      selection = result.current.select(artifact("a.pdf"));
    });
    await act(async () => {
      pending.resolve(file("blob:uncommitted"));
      await selection;
      unmount();
    });
    expect(URL.revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:uncommitted");
  });

  it("discards downloads completing after unmount", async () => {
    const pending = deferred<ReturnType<typeof file>>();
    download.mockReturnValue(pending.promise);
    const { result, unmount } = renderHook(() => useWorkspacePreview("a", true));
    let selection!: Promise<void>;
    act(() => {
      selection = result.current.select(artifact("a.pdf"));
    });
    unmount();
    await act(async () => {
      pending.resolve(file("blob:unmounted"));
      await selection;
    });
    expect(URL.revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:unmounted");
  });

  it("invalidates pending text decoding without double revocation", async () => {
    const text = deferred<string>();
    const response = new Response();
    vi.spyOn(response, "text").mockReturnValue(text.promise);
    vi.spyOn(globalThis, "fetch").mockResolvedValue(response);
    download.mockResolvedValue(file("blob:text", "text/plain"));
    const { result } = renderHook(() => useWorkspacePreview("a", true));
    let selection!: Promise<void>;
    await act(async () => {
      selection = result.current.select(artifact("a.txt"));
      await Promise.resolve();
    });
    act(() => result.current.close());
    expect(URL.revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:text");
    await act(async () => {
      text.resolve("old text");
      await selection;
    });
    expect(result.current.state).toBeNull();
    expect(URL.revokeObjectURL).toHaveBeenCalledTimes(1);
  });

  it("preserves page and URL through expansion and resets on replacement", async () => {
    download.mockResolvedValueOnce(file("blob:a")).mockResolvedValueOnce(file("blob:b"));
    const { result } = renderHook(() => useWorkspacePreview("a", true));
    await act(async () => result.current.select(artifact("a.pdf")));
    act(() => result.current.pdf.onDocumentLoad(5));
    act(() => result.current.pdf.onPageChange(3));
    act(() => result.current.setDialogOpen(true));
    act(() => result.current.pdf.onPageChange(4));
    act(() => result.current.setDialogOpen(false));
    expect(result.current.pdf.page).toBe(4);
    expect(result.current.state?.url).toBe("blob:a");
    expect(download).toHaveBeenCalledTimes(1);
    expect(URL.revokeObjectURL).not.toHaveBeenCalled();
    await act(async () => result.current.select(artifact("b.pdf")));
    expect(result.current.pdf.page).toBe(1);
    expect(result.current.pdf.pageCount).toBeNull();
    expect(URL.revokeObjectURL).toHaveBeenCalledExactlyOnceWith("blob:a");
  });
});
