import { ChevronDown, ChevronRight } from "lucide-react";
import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";

import type { ToolCallStep } from "@/views/ask/hooks/use-ask-question";
import { ThinkingStep } from "./thinking-step";

export const ThinkingSteps = ({
  steps,
  defaultOpen = true,
}: {
  steps: ToolCallStep[];
  defaultOpen?: boolean;
}): React.ReactElement | null => {
  const [open, setOpen] = useState(defaultOpen);

  if (steps.length === 0) return null;

  const running = steps.filter((s) => s.status === "running").length;

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger className="flex w-full items-center gap-1 text-sm font-medium text-muted-foreground hover:text-foreground">
        {open ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
        Tool calls
        <Badge variant="secondary" className="ml-1">
          {steps.length}
        </Badge>
        {running > 0 && (
          <Badge variant="outline" className="ml-1">
            {running} running
          </Badge>
        )}
      </CollapsibleTrigger>
      <CollapsibleContent className="mt-1 space-y-0.5 border-l-2 border-border pl-3">
        {steps.map((step, i) => (
          <ThinkingStep key={`${step.toolName}-${i}`} step={step} />
        ))}
      </CollapsibleContent>
    </Collapsible>
  );
};
