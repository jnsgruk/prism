import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { ChevronDown } from "lucide-react";

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const readableCoverage = (message: string): string =>
  message.replace(/^visible_activity_only:\s*/i, "Visible activity only: ");

export const RunCoverage = ({ progressJson }: { progressJson?: string }): React.ReactElement | null => {
  let progress: unknown;
  try {
    progress = JSON.parse(progressJson ?? "null") as unknown;
  } catch {
    return null;
  }
  if (!isRecord(progress)) return null;

  const coverage = (Array.isArray(progress.coverage) ? progress.coverage : [progress.coverage])
    .filter((value): value is string => typeof value === "string" && value.trim().length > 0)
    .map(readableCoverage);
  const failures = (Array.isArray(progress.failed_items) ? progress.failed_items : []).filter(isRecord);
  if (!coverage.length && !failures.length) return null;

  return (
    <div className="min-w-0 space-y-2 text-xs">
      {coverage.length > 0 && (
        <Alert className="min-w-0">
          <AlertTitle>Activity coverage</AlertTitle>
          <AlertDescription className="space-y-1 break-words [overflow-wrap:anywhere]">
            {coverage.map((message, index) => (
              <p key={`${index}-${message}`}>{message}</p>
            ))}
          </AlertDescription>
        </Alert>
      )}
      {failures.length > 0 && (
        <Collapsible>
          <CollapsibleTrigger
            render={<Button variant="ghost" size="sm" className="h-auto max-w-full whitespace-normal text-left" />}
          >
            <ChevronDown className="mr-1 size-3 shrink-0" />
            {failures.length.toLocaleString()} {failures.length === 1 ? "item could" : "items could"} not be collected
          </CollapsibleTrigger>
          <CollapsibleContent className="max-h-48 space-y-2 overflow-y-auto break-words px-2 py-1 [overflow-wrap:anywhere]">
            {failures.map((failure, index) => (
              <div key={index}>
                {typeof failure.key === "string" && <p className="font-medium">{failure.key}</p>}
                <p className="text-muted-foreground">
                  {typeof failure.error === "string" ? failure.error : "Collection was incomplete for this item."}
                </p>
              </div>
            ))}
          </CollapsibleContent>
        </Collapsible>
      )}
    </div>
  );
};
