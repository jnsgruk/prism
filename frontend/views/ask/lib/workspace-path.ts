/** Decode URL path segments exactly once, rejecting ambiguous filesystem paths. */
export const decodeWorkspacePath = (encoded: string): string | null => {
  try {
    if (encoded.length > 12288) return null;
    const segments = encoded.split("/").map((segment) => decodeURIComponent(segment));
    if (
      segments.some(
        (segment) =>
          !segment ||
          segment === "." ||
          segment === ".." ||
          /[/\\]/.test(segment) ||
          segment
            .split("")
            .some(
              (character) =>
                character.charCodeAt(0) < 32 || (character.charCodeAt(0) >= 127 && character.charCodeAt(0) <= 159),
            ),
      )
    ) {
      return null;
    }
    const path = segments.join("/");
    return new TextEncoder().encode(path).length <= 4096 ? path : null;
  } catch {
    return null;
  }
};

const FILE_EXTENSION = /\.(pdf|csv|tsv|txt|md|json|zip|docx?|xlsx?|pptx?|html?|png|jpe?g|gif|webp|svg|bmp)$/i;

/** Remote URLs never become local downloads, even when their path names match. */
export const classifyWorkspaceReference = (href: string, conversationId?: string): { path: string | null } | null => {
  let reference = href;
  const canonical = /^\/ask\/([^/]+)\/files\/(.*)$/.exec(reference);
  if (canonical) {
    const matchesConversation = conversationId && canonical[1] === encodeURIComponent(conversationId);
    return { path: matchesConversation ? decodeWorkspacePath(canonical[2]!) : null };
  }
  if (/^https?:\/\//i.test(reference)) {
    let url: URL;
    try {
      url = new URL(reference);
    } catch {
      return null;
    }
    if (url.origin !== window.location.origin || !url.pathname.startsWith("/workspace/")) return null;
    reference = url.pathname;
  }

  if (reference.startsWith("/workspace/")) return { path: decodeWorkspacePath(reference.slice(11)) };
  if (reference.startsWith("workspace/")) return { path: decodeWorkspacePath(reference.slice(10)) };
  if (reference.startsWith("./")) reference = reference.slice(2);
  if (reference.startsWith("/") || /^[a-z][a-z\d+.-]*:/i.test(reference) || /[?#]/.test(reference)) return null;
  const path = decodeWorkspacePath(reference);
  return FILE_EXTENSION.test(path ?? reference) ? { path } : null;
};

export const workspaceDownloadHref = (conversationId: string, path: string): string =>
  `/ask/${encodeURIComponent(conversationId)}/files/${path.split("/").map(encodeURIComponent).join("/")}`;

export const saveWorkspaceDownload = (blobUrl: string, filename: string): void => {
  const anchor = document.createElement("a");
  anchor.href = blobUrl;
  anchor.download = filename;
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
  // Give the browser time to begin consuming the Blob before releasing it.
  setTimeout(() => URL.revokeObjectURL(blobUrl), 1000);
};
