import type { LucideIcon } from "lucide-react";
import {
  Check,
  Database,
  FileText,
  FolderSearch,
  Loader2,
  Search,
  Terminal,
  Upload,
  X,
} from "lucide-react";

import type { ToolCallStep } from "@/views/ask/hooks/use-ask-question";

const toolIcon = (toolName: string): LucideIcon => {
  if (toolName.startsWith("mcp_prism") || toolName.startsWith("prism_")) return Database;
  if (toolName === "bash") return Terminal;
  if (toolName === "read" || toolName === "write" || toolName === "edit" || toolName === "patch")
    return FileText;
  if (toolName === "glob") return FolderSearch;
  if (toolName === "grep") return Search;
  if (toolName === "upload_artifact") return Upload;
  return Terminal;
};

const toolLabel = (step: ToolCallStep): string => {
  if (step.toolName === "bash") {
    try {
      const args = JSON.parse(step.argumentsJson);
      const cmd = args.command ?? args.cmd ?? "";
      return cmd.length > 60 ? `${cmd.slice(0, 60)}...` : cmd;
    } catch {
      return "bash";
    }
  }
  return step.toolName.replace(/^mcp_prism_/, "").replace(/_/g, " ");
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

export const ThinkingStep = ({ step }: { step: ToolCallStep }): React.ReactElement => {
  const Icon = toolIcon(step.toolName);

  return (
    <div className="flex items-start gap-2 py-1 text-sm">
      <Icon className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" />
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-1.5">
          <span className={step.toolName === "bash" ? "font-mono text-xs" : ""}>
            {toolLabel(step)}
          </span>
          {statusIcon(step.status)}
          {step.durationMs != null && step.status !== "running" && (
            <span className="text-xs text-muted-foreground">
              {step.durationMs < 1000
                ? `${step.durationMs}ms`
                : `${(step.durationMs / 1000).toFixed(1)}s`}
            </span>
          )}
        </div>
        {step.resultSummary && step.status !== "running" && (
          <p className="mt-0.5 truncate text-xs text-muted-foreground">{step.resultSummary}</p>
        )}
      </div>
    </div>
  );
};
