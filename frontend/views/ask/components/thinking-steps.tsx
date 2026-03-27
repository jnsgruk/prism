import { ChevronDown, ChevronRight, Loader2 } from "lucide-react";
import { useEffect, useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";

import type { AgentStep } from "@/views/ask/hooks/use-ask-question";
import { ThinkingStep } from "./thinking-step";

export const ThinkingSteps = ({
  steps,
  defaultOpen = true,
}: {
  steps: AgentStep[];
  defaultOpen?: boolean;
}): React.ReactElement | null => {
  const [open, setOpen] = useState(defaultOpen);

  useEffect(() => {
    setOpen(defaultOpen);
  }, [defaultOpen]);

  if (steps.length === 0) return null;

  const toolSteps = steps.filter((s) => s.kind === "tool");
  const running = toolSteps.filter((s) => s.status === "running").length;
  const isActive = running > 0 || defaultOpen;

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger className="flex w-full items-center gap-1.5 text-sm font-medium text-muted-foreground hover:text-foreground">
        {open ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
        {isActive ? (
          <>
            <Loader2 className="size-3.5 animate-spin" />
            Working
          </>
        ) : (
          "Agent activity"
        )}
        {toolSteps.length > 0 && (
          <Badge variant="secondary" className="ml-1">
            {toolSteps.length} tool call{toolSteps.length !== 1 && "s"}
          </Badge>
        )}
      </CollapsibleTrigger>
      <CollapsibleContent className="mt-1 space-y-0.5 border-l-2 border-border pl-3">
        {steps.map((step, i) => (
          <ThinkingStep
            key={step.stepId ?? (step.kind === "tool" ? step.callId : `reasoning-${i}`)}
            step={step}
          />
        ))}
      </CollapsibleContent>
    </Collapsible>
  );
};
