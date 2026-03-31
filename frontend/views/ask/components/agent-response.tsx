import { Badge } from "@/components/ui/badge";
import { Sparkles } from "lucide-react";

import type { AgentState, AgentStep } from "@/views/ask/hooks/use-ask-question";
import type { ArtifactInfo } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { AnswerContent } from "./answer-content";
import { ArtifactList } from "./artifact-list";
import { EvidencePanel } from "./evidence-panel";
import { InlineImage } from "./inline-image";
import { ThinkingSteps } from "./thinking-steps";

const isImageArtifact = (a: ArtifactInfo): boolean => a.contentType?.startsWith("image/") ?? false;

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
  // Filter out echoed question text that OpenCode sends as the first Part::Text
  const isEchoedQuestion = question && answer.trim() === question.trim();
  const displayAnswer = isEchoedQuestion ? "" : answer;

  const imageArtifacts = artifacts.filter(isImageArtifact);
  const otherArtifacts = artifacts.filter((a) => !isImageArtifact(a));

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

        {imageArtifacts.map((a) => (
          <InlineImage key={a.id} artifact={a} />
        ))}

        {displayAnswer && <AnswerContent content={displayAnswer} />}

        {otherArtifacts.length > 0 && <ArtifactList artifacts={otherArtifacts} />}

        {state.status === "completed" && supportingData && (
          <EvidencePanel supportingData={supportingData} />
        )}
      </div>
    </div>
  );
};
