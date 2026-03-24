import { ChevronDown, ChevronRight, Eye } from "lucide-react";
import { useState } from "react";

import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";

import type { AgentStep } from "@/views/ask/hooks/use-ask-question";
import { ThinkingSteps } from "./thinking-steps";

export const EvidencePanel = ({
  steps,
  supportingData,
}: {
  steps: AgentStep[];
  supportingData?: string;
}): React.ReactElement => {
  const [open, setOpen] = useState(false);

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <CollapsibleTrigger className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground hover:text-foreground">
        <Eye className="size-3.5" />
        Evidence & Reasoning
        {open ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
      </CollapsibleTrigger>
      <CollapsibleContent className="mt-2 space-y-3 rounded-md border bg-muted/30 p-3">
        <ThinkingSteps steps={steps} defaultOpen />
        {supportingData && supportingData !== "{}" && supportingData !== "null" && (
          <div className="space-y-1">
            <p className="text-xs font-medium text-muted-foreground">Supporting data</p>
            <pre className="overflow-x-auto rounded bg-muted p-2 text-xs">
              {JSON.stringify(JSON.parse(supportingData), null, 2)}
            </pre>
          </div>
        )}
      </CollapsibleContent>
    </Collapsible>
  );
};
