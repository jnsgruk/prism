import { WorkspaceFileLink } from "@/views/ask/components/workspace-file-link";
import { WorkspaceImage } from "@/views/ask/components/workspace-image";
import { classifyWorkspaceReference } from "@/views/ask/lib/workspace-path";
import Markdown from "react-markdown";
import { Link } from "react-router-dom";
import remarkGfm from "remark-gfm";

const INTERNAL_LINK_RE = /^\/(teams|people|contributions|ingestion|ask|admin)/;

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
          const reference = href ? classifyWorkspaceReference(href, conversationId) : null;
          if (reference)
            return (
              <WorkspaceFileLink conversationId={conversationId} path={reference.path}>
                {children}
              </WorkspaceFileLink>
            );
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
          const reference = src ? classifyWorkspaceReference(src, conversationId) : null;
          if (reference?.path && conversationId) {
            return <WorkspaceImage conversationId={conversationId} path={reference.path} alt={alt ?? undefined} />;
          }
          // Fall back to a normal <img> for absolute URLs / data URIs.
          return <img src={src} alt={alt ?? ""} className="max-h-[500px] rounded-md" />;
        },
        pre: ({ children, ...props }) => (
          <pre className="overflow-x-auto rounded-md bg-muted p-3 text-sm text-foreground" {...props}>
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
