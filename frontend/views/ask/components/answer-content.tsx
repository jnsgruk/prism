import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Link } from "react-router-dom";

const INTERNAL_LINK_RE = /^\/(teams|people|contributions|ingestion|ask|admin)/;

export const AnswerContent = ({ content }: { content: string }): React.ReactElement => (
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
