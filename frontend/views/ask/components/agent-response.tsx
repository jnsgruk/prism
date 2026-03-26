import { Badge } from "@/components/ui/badge";
import { Sparkles } from "lucide-react";

import type { AgentState, AgentStep } from "@/views/ask/hooks/use-ask-question";
import type { ArtifactInfo } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { AnswerContent } from "./answer-content";
import { ArtifactList } from "./artifact-list";
import { EvidencePanel } from "./evidence-panel";
import { ThinkingSteps } from "./thinking-steps";

const TokenSummary = ({
  promptTokens,
  completionTokens,
  estimatedCostUsd,
  durationMs,
  toolCallCount,
}: {
  promptTokens: number;
  completionTokens: number;
  estimatedCostUsd: number;
  durationMs: number;
  toolCallCount: number;
}): React.ReactElement => (
  <div className="flex flex-wrap gap-2 text-xs text-muted-foreground">
    <span>{toolCallCount} tool calls</span>
    <span>{promptTokens + completionTokens} tokens</span>
    <span>${estimatedCostUsd.toFixed(4)}</span>
    <span>{(durationMs / 1000).toFixed(1)}s</span>
  </div>
);

export const AgentResponse = ({
  state,
  steps,
  answer,
  question,
  artifacts,
  supportingData,
}: {
  state: AgentState;
  steps: AgentStep[];
  answer: string;
  question?: string;
  artifacts: ArtifactInfo[];
  supportingData?: string;
}): React.ReactElement => {
  const toolCallCount = steps.filter((s) => s.kind === "tool").length;
  // Filter out echoed question text that OpenCode sends as the first Part::Text
  const isEchoedQuestion = question && answer.trim() === question.trim();
  const displayAnswer = isEchoedQuestion ? "" : answer;

  return (
    <div className="flex gap-3">
      <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-violet-100 text-violet-700 dark:bg-violet-900/30 dark:text-violet-400">
        <Sparkles className="size-3.5" />
      </div>
      <div className="min-w-0 flex-1 space-y-3 pt-0.5">
        {steps.length > 0 && (
          <ThinkingSteps steps={steps} defaultOpen={state.status === "streaming"} />
        )}

        {state.status === "streaming" && !displayAnswer && steps.length === 0 && (
          <Badge variant="secondary" className="animate-pulse">
            Thinking...
          </Badge>
        )}

        {displayAnswer && <AnswerContent content={displayAnswer} />}

        {artifacts.length > 0 && <ArtifactList artifacts={artifacts} />}

        {state.status === "completed" && (
          <>
            <EvidencePanel supportingData={supportingData} />
            <TokenSummary
              promptTokens={state.tokenUsage.promptTokens}
              completionTokens={state.tokenUsage.completionTokens}
              estimatedCostUsd={state.tokenUsage.estimatedCostUsd}
              durationMs={state.durationMs}
              toolCallCount={toolCallCount}
            />
          </>
        )}
      </div>
    </div>
  );
};
