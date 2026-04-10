import { resolveLanguage } from "@/views/ask/hooks/use-file-tree";
import { Loader2 } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { type BundledLanguage, type Highlighter, createHighlighter } from "shiki";

// ---------------------------------------------------------------------------
// Singleton highlighter — lazily initialised once
// ---------------------------------------------------------------------------

let highlighterPromise: Promise<Highlighter> | null = null;
const loadedLanguages = new Set<string>(["text"]);
const failedLanguages = new Set<string>();

const getHighlighter = (): Promise<Highlighter> => {
  if (!highlighterPromise) {
    highlighterPromise = createHighlighter({
      themes: ["github-dark", "github-light"],
      langs: [],
    });
  }
  return highlighterPromise;
};

const ensureLanguage = async (highlighter: Highlighter, lang: string): Promise<void> => {
  if (loadedLanguages.has(lang) || failedLanguages.has(lang)) return;
  try {
    await highlighter.loadLanguage(lang as BundledLanguage);
    loadedLanguages.add(lang);
  } catch {
    // Language not available — track separately so codeToHtml falls back to "text".
    failedLanguages.add(lang);
  }
};

// ---------------------------------------------------------------------------
// Component
// ---------------------------------------------------------------------------

export const CodePreview = ({
  code,
  fileName,
  contentType,
  className,
}: {
  code: string;
  fileName: string;
  contentType?: string;
  className?: string;
}): React.ReactElement => {
  const [html, setHtml] = useState<string | null>(null);
  const containerRef = useRef<HTMLDivElement>(null);

  const language = resolveLanguage(fileName, contentType);

  useEffect(() => {
    let cancelled = false;

    const highlight = async (): Promise<void> => {
      if (!language) {
        // No known language — render as plain text, no highlighting
        setHtml(null);
        return;
      }

      const highlighter = await getHighlighter();
      await ensureLanguage(highlighter, language);

      if (cancelled) return;

      const result = highlighter.codeToHtml(code, {
        lang: loadedLanguages.has(language) && !failedLanguages.has(language) ? language : "text",
        themes: {
          light: "github-light",
          dark: "github-dark",
        },
        defaultColor: false,
      });
      setHtml(result);
    };

    highlight();
    return (): void => {
      cancelled = true;
    };
  }, [code, language]);

  // Plain text fallback for unknown types
  if (!language) {
    return (
      <pre className={`overflow-auto rounded-md bg-muted p-3 text-sm ${className ?? ""}`}>
        <code>{code}</code>
      </pre>
    );
  }

  if (html === null) {
    return (
      <div className={`flex items-center justify-center p-4 ${className ?? ""}`}>
        <Loader2 className="size-4 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div
      ref={containerRef}
      className={`overflow-auto rounded-md text-sm [&_pre]:!overflow-visible [&_pre]:rounded-md [&_pre]:p-3 ${className ?? ""}`}
      dangerouslySetInnerHTML={{ __html: html }}
    />
  );
};
