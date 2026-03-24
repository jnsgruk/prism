import { Sparkles } from "lucide-react";

import { Button } from "@/components/ui/button";

const SUGGESTIONS = [
  "Which teams have the highest merge throughput this month?",
  "Show me contributors who have been most active across repositories",
  "What is the average time from PR open to merge for each team?",
  "Which repositories have the most stale open PRs?",
];

export const SuggestedQuestions = ({
  onSelect,
}: {
  onSelect: (question: string) => void;
}): React.ReactElement => (
  <div className="flex flex-col items-center justify-center space-y-6 py-12">
    <div className="flex flex-col items-center gap-2">
      <Sparkles className="size-10 text-muted-foreground" />
      <h2 className="text-lg font-medium">Ask Prism</h2>
      <p className="text-sm text-muted-foreground">
        Ask questions about your engineering data in natural language.
      </p>
    </div>
    <div className="grid max-w-2xl gap-2 sm:grid-cols-2">
      {SUGGESTIONS.map((q) => (
        <Button
          key={q}
          variant="outline"
          className="h-auto justify-start whitespace-normal px-4 py-3 text-left text-sm"
          onClick={() => onSelect(q)}
        >
          {q}
        </Button>
      ))}
    </div>
  </div>
);
