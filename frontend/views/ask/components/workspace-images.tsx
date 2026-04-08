import { useMemo } from "react";

import type { AgentStep } from "@/views/ask/hooks/use-ask-question";
import { WorkspaceImage } from "@/views/ask/components/workspace-image";

/** Image extensions we display inline. */
const IMAGE_EXT_RE = /\.(png|jpe?g|gif|webp|svg|bmp)$/i;

/** Match workspace image paths in arbitrary text (tool args, results, content). */
const WORKSPACE_PATH_RE = /\/workspace\/([\w./-]+\.(?:png|jpe?g|gif|webp|svg|bmp))/gi;

/** Normalise a workspace path — strip /workspace/ prefix and leading slash. */
const normalise = (p: string): string => {
  let s = p;
  if (s.startsWith("/workspace/")) s = s.slice("/workspace/".length);
  if (s.startsWith("workspace/")) s = s.slice("workspace/".length);
  if (s.startsWith("/")) s = s.slice(1);
  return s;
};

/**
 * Extract workspace image paths already referenced in markdown content
 * via `![alt](path)` syntax so we don't duplicate them.
 */
const extractMarkdownImages = (markdown: string): Set<string> => {
  const refs = new Set<string>();
  const re = /!\[[^\]]*\]\(([^)]+)\)/g;
  let match;
  while ((match = re.exec(markdown)) !== null) {
    if (IMAGE_EXT_RE.test(match[1]!)) {
      refs.add(normalise(match[1]!));
    }
  }
  return refs;
};

/**
 * Extract image paths mentioned in tool call traces (bash commands, write
 * operations, result summaries). This finds images the agent produced even
 * when it doesn't reference them in markdown syntax.
 */
const extractImagePathsFromSteps = (steps: AgentStep[]): string[] => {
  const paths = new Set<string>();

  for (const step of steps) {
    if (step.kind !== "tool") continue;

    // Scan arguments (e.g. bash command containing an output path,
    // or write tool targeting an image file).
    if (step.argumentsJson) {
      for (const m of step.argumentsJson.matchAll(WORKSPACE_PATH_RE)) {
        paths.add(normalise(m[0]));
      }
    }

    // Scan result summary for workspace image paths.
    if (step.resultSummary) {
      for (const m of step.resultSummary.matchAll(WORKSPACE_PATH_RE)) {
        paths.add(normalise(m[0]));
      }
    }
  }

  return [...paths];
};

/**
 * Shows workspace images that were produced during a specific message's
 * tool calls and are NOT already rendered inline via markdown image syntax.
 * This associates images with the message that created them.
 */
export const WorkspaceImages = ({
  conversationId,
  steps,
  answerContent,
}: {
  conversationId: string;
  /** Tool call steps from this specific message's reasoning trace. */
  steps: AgentStep[];
  /** The assistant's markdown text — used to deduplicate already-inlined images. */
  answerContent: string;
}): React.ReactElement | null => {
  const imagePaths = useMemo(() => {
    const markdownRefs = extractMarkdownImages(answerContent);
    return extractImagePathsFromSteps(steps).filter((p) => !markdownRefs.has(p));
  }, [steps, answerContent]);

  if (imagePaths.length === 0) return null;

  return (
    <div className="space-y-3">
      {imagePaths.map((path) => (
        <WorkspaceImage
          key={path}
          conversationId={conversationId}
          path={path}
          alt={path.split("/").pop()}
        />
      ))}
    </div>
  );
};
