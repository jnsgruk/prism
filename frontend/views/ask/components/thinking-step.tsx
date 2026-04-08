import { useState } from "react";
import type { LucideIcon } from "lucide-react";
import {
  Check,
  ChevronRight,
  Database,
  FileText,
  FolderSearch,
  Loader2,
  Search,
  Terminal,
  X,
} from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";

import type { AgentStep, ToolCallStep } from "@/views/ask/hooks/use-ask-question";

const toolIcon = (toolName: string): LucideIcon => {
  if (toolName.startsWith("mcp_prism") || toolName.startsWith("prism_")) return Database;
  if (toolName === "bash") return Terminal;
  if (toolName === "read" || toolName === "write" || toolName === "edit" || toolName === "patch")
    return FileText;
  if (toolName === "glob") return FolderSearch;
  if (toolName === "grep") return Search;
  return Terminal;
};

/** Try to parse argumentsJson, handling double-encoded strings. */
const parseArgs = (json: string): Record<string, unknown> | null => {
  try {
    let parsed = JSON.parse(json);
    // Handle double-encoded JSON (string containing JSON).
    if (typeof parsed === "string") parsed = JSON.parse(parsed);
    return typeof parsed === "object" && parsed !== null ? parsed : null;
  } catch {
    return null;
  }
};

const truncLabel = (s: string, max = 80): string => (s.length > max ? `${s.slice(0, max)}...` : s);

const toolLabel = (step: ToolCallStep): string => {
  const args = parseArgs(step.argumentsJson);
  const name = step.toolName.replace(/^prism_/, "").replace(/_/g, " ");

  if (!args) return name;

  switch (step.toolName) {
    case "bash": {
      const cmd = (args.command ?? args.cmd ?? args.input) as string | undefined;
      return cmd ? truncLabel(cmd) : "bash";
    }
    case "write":
    case "read":
    case "edit":
    case "patch": {
      const path = (args.file_path ?? args.filePath ?? args.path) as string | undefined;
      if (!path) return name;
      // Show just the filename for short labels.
      const filename = path.split("/").pop() ?? path;
      return `${name} ${filename}`;
    }
    case "glob": {
      const pattern = (args.pattern ?? args.glob) as string | undefined;
      return pattern ? `glob ${truncLabel(pattern, 60)}` : name;
    }
    case "grep": {
      const pattern = (args.pattern ?? args.regex) as string | undefined;
      return pattern ? `grep ${truncLabel(pattern, 60)}` : name;
    }
    default:
      return name;
  }
};

const statusIcon = (status: ToolCallStep["status"]): React.ReactElement => {
  if (status === "running") {
    return <Loader2 className="size-3.5 animate-spin text-muted-foreground" />;
  }
  if (status === "completed") {
    return <Check className="size-3.5 text-green-600" />;
  }
  return <X className="size-3.5 text-destructive" />;
};

const ToolStep = ({ step }: { step: ToolCallStep }): React.ReactElement => {
  const [expanded, setExpanded] = useState(false);
  const Icon = toolIcon(step.toolName);
  const hasDetails = !!step.resultSummary && step.status !== "running";

  return (
    <div className="py-1 text-sm">
      <button
        type="button"
        className="flex w-full items-center gap-2 text-left"
        onClick={() => hasDetails && setExpanded((v) => !v)}
        disabled={!hasDetails}
      >
        <Icon className="size-3.5 shrink-0 text-muted-foreground" />
        <span
          className={`min-w-0 truncate ${["bash", "write", "read", "edit", "patch", "glob", "grep"].includes(step.toolName) ? "font-mono text-xs" : ""}`}
        >
          {toolLabel(step)}
        </span>
        {statusIcon(step.status)}
        {step.durationMs != null && step.status !== "running" && (
          <span className="shrink-0 text-xs text-muted-foreground">
            {step.durationMs < 1000
              ? `${step.durationMs}ms`
              : `${(step.durationMs / 1000).toFixed(1)}s`}
          </span>
        )}
        {hasDetails && (
          <ChevronRight
            className={`size-3 shrink-0 text-muted-foreground transition-transform ${expanded ? "rotate-90" : ""}`}
          />
        )}
      </button>
      {expanded && step.resultSummary && (
        <pre className="mt-1 ml-5.5 max-h-64 overflow-auto rounded bg-muted p-2 font-mono text-xs text-muted-foreground">
          {step.resultSummary}
        </pre>
      )}
    </div>
  );
};

export const ThinkingStep = ({ step }: { step: AgentStep }): React.ReactElement => {
  if (step.kind === "reasoning") {
    return (
      <div className="prose prose-sm dark:prose-invert max-w-none py-1 text-muted-foreground">
        <Markdown remarkPlugins={[remarkGfm]}>{step.text}</Markdown>
      </div>
    );
  }
  return <ToolStep step={step} />;
};
