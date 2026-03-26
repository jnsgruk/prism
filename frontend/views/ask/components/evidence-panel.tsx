import { ChevronDown, ChevronRight, Eye } from "lucide-react";
import { useState } from "react";

import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";

export const EvidencePanel = ({
  supportingData,
}: {
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
