import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Link } from "react-router-dom";

import { WorkspaceImage } from "./workspace-image";

const INTERNAL_LINK_RE = /^\/(teams|people|contributions|ingestion|ask|admin)/;

/** Image file extensions the agent typically generates. */
const IMAGE_EXT_RE = /\.(png|jpe?g|gif|webp|svg|bmp)$/i;

/**
 * Check whether an image src looks like a workspace-relative path
 * (not an absolute URL). The agent writes files to /workspace and references
 * them in markdown as e.g. `![chart](chart.png)` or `![img](/workspace/output.png)`.
 */
const isWorkspacePath = (src: string): boolean => {
  // Absolute URLs or data URIs are not workspace paths.
  if (/^https?:\/\//i.test(src) || src.startsWith("data:")) return false;
  return IMAGE_EXT_RE.test(src);
};

/** Normalise workspace paths — strip leading /workspace/ prefix if present. */
const normaliseWorkspacePath = (src: string): string => {
  let p = src;
  // Strip leading slash for relative resolution.
  if (p.startsWith("/workspace/")) p = p.slice("/workspace/".length);
  if (p.startsWith("workspace/")) p = p.slice("workspace/".length);
  if (p.startsWith("/")) p = p.slice(1);
  return p;
};

export const AnswerContent = ({
  content,
  conversationId,
}: {
  content: string;
  conversationId?: string;
}): React.ReactElement => (
  <div className="prose prose-sm dark:prose-invert max-w-none">
    <Markdown
      remarkPlugins={[remarkGfm]}
      components={{
        a: ({ href, children, ...props }) => {
          if (href && INTERNAL_LINK_RE.test(href)) {
            return <Link to={href}>{children}</Link>;
          }
          return (
            <a href={href} target="_blank" rel="noopener noreferrer" {...props}>
              {children}
            </a>
          );
        },
        img: ({ src, alt }) => {
          if (src && conversationId && isWorkspacePath(src)) {
            return (
              <WorkspaceImage
                conversationId={conversationId}
                path={normaliseWorkspacePath(src)}
                alt={alt ?? undefined}
              />
            );
          }
          // Fall back to a normal <img> for absolute URLs / data URIs.
          return <img src={src} alt={alt ?? ""} className="max-h-[500px] rounded-md" />;
        },
        pre: ({ children, ...props }) => (
          <pre
            className="overflow-x-auto rounded-md bg-muted p-3 text-sm text-foreground"
            {...props}
          >
            {children}
          </pre>
        ),
        code: ({ children, className, ...props }) => {
          const isBlock = className?.startsWith("language-");
          if (isBlock)
            return (
              <code className={className} {...props}>
                {children}
              </code>
            );
          return (
            <code className="rounded bg-muted px-1 py-0.5 text-sm text-foreground" {...props}>
              {children}
            </code>
          );
        },
        table: ({ children, ...props }) => (
          <div className="overflow-x-auto">
            <table className="text-sm" {...props}>
              {children}
            </table>
          </div>
        ),
      }}
    >
      {content}
    </Markdown>
  </div>
);
