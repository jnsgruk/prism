import { Badge } from "@/components/ui/badge";
import { Sparkles } from "lucide-react";

import type { AgentState, AgentStep } from "@/views/ask/hooks/use-ask-question";
import type { WorkspaceFileDisplay } from "@/views/ask/hooks/use-file-tree";
import { AnswerContent } from "./answer-content";
import { EvidencePanel } from "./evidence-panel";
import { ThinkingSteps } from "./thinking-steps";
import { WorkspaceImages } from "./workspace-images";

export const AgentResponse = ({
  state,
  steps,
  answer,
  question,
  supportingData,
  conversationId,
  workspaceFiles,
}: {
  state: AgentState;
  steps: AgentStep[];
  answer: string;
  question?: string;
  supportingData?: string;
  conversationId?: string;
  workspaceFiles: WorkspaceFileDisplay[];
}): React.ReactElement => {
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

        {displayAnswer && <AnswerContent content={displayAnswer} conversationId={conversationId} />}

        {state.status === "completed" && conversationId && (
          <WorkspaceImages
            conversationId={conversationId}
            workspaceFiles={workspaceFiles}
            answerContent={displayAnswer}
          />
        )}

        {state.status === "completed" && supportingData && (
          <EvidencePanel supportingData={supportingData} />
        )}
      </div>
    </div>
  );
};
