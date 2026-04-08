import { useMemo } from "react";

import type { WorkspaceFileDisplay } from "@/views/ask/hooks/use-file-tree";
import { WorkspaceImage } from "@/views/ask/components/workspace-image";

/** Image extensions we display inline. */
const IMAGE_EXT_RE = /\.(png|jpe?g|gif|webp|svg|bmp)$/i;

/**
 * Extract workspace image paths already referenced in markdown content
 * via `![alt](path)` syntax so we don't duplicate them.
 */
const extractReferencedImages = (markdown: string): Set<string> => {
  const refs = new Set<string>();
  // Match markdown image syntax: ![...](path)
  const re = /!\[[^\]]*\]\(([^)]+)\)/g;
  let match;
  while ((match = re.exec(markdown)) !== null) {
    let src = match[1]!;
    // Normalise: strip /workspace/ prefix, leading slash.
    if (src.startsWith("/workspace/")) src = src.slice("/workspace/".length);
    if (src.startsWith("workspace/")) src = src.slice("workspace/".length);
    if (src.startsWith("/")) src = src.slice(1);
    refs.add(src);
  }
  return refs;
};

/**
 * Shows workspace image files that are NOT already referenced in the
 * assistant's markdown response. This ensures generated images (e.g. from
 * Python matplotlib scripts) always appear in the chat, even when the agent
 * doesn't use markdown image syntax.
 */
export const WorkspaceImages = ({
  conversationId,
  workspaceFiles,
  answerContent,
}: {
  conversationId: string;
  workspaceFiles: WorkspaceFileDisplay[];
  /** The assistant's markdown text — used to deduplicate already-inlined images. */
  answerContent: string;
}): React.ReactElement | null => {
  const unreferencedImages = useMemo(() => {
    const referenced = extractReferencedImages(answerContent);
    return workspaceFiles.filter((f) => {
      if (f.isDirectory) return false;
      if (!IMAGE_EXT_RE.test(f.path)) return false;
      // Check if this path is already referenced in the markdown.
      // Compare against both the full path and just the filename.
      const normalised = f.path.startsWith("/") ? f.path.slice(1) : f.path;
      const fileName = f.path.split("/").pop() ?? f.path;
      return !referenced.has(normalised) && !referenced.has(fileName);
    });
  }, [workspaceFiles, answerContent]);

  if (unreferencedImages.length === 0) return null;

  return (
    <div className="space-y-3">
      {unreferencedImages.map((file) => (
        <WorkspaceImage
          key={file.path}
          conversationId={conversationId}
          path={file.path}
          alt={file.path.split("/").pop()}
        />
      ))}
    </div>
  );
};
