import { Badge } from "@/components/ui/badge";
import { ExternalLink } from "lucide-react";

import type { NotableContribution } from "@ps/api/gen/canonical/prism/v1/insights_pb";

const enrichmentLabel = (type: string): string => {
  switch (type) {
    case "review_depth":
      return "Deep review";
    case "significance":
      return "Significant PR";
    default:
      return type;
  }
};

export const NotableContributionCard = ({
  item,
}: {
  item: NotableContribution;
}): React.ReactElement => (
  <div className="rounded-lg border bg-muted/30 p-4">
    <div className="mb-2 flex flex-wrap items-center gap-2">
      <Badge variant="secondary" className="text-[10px] uppercase">
        {enrichmentLabel(item.enrichmentType)}
      </Badge>
      {item.confidence > 0 && (
        <span className="text-[10px] tabular-nums text-muted-foreground">
          {Math.round(item.confidence * 100)}% confidence
        </span>
      )}
    </div>
    <p className="mb-2 text-sm italic text-muted-foreground leading-relaxed">
      &ldquo;{item.rationale}&rdquo;
    </p>
    <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
      <span className="font-medium text-foreground">{item.personName}</span>
      {item.title && (
        <>
          <span>&mdash;</span>
          {item.url ? (
            <a
              href={item.url}
              target="_blank"
              rel="noopener noreferrer"
              className="inline-flex items-center gap-1 underline-offset-4 hover:underline"
            >
              {item.title}
              <ExternalLink className="size-3" />
            </a>
          ) : (
            <span>{item.title}</span>
          )}
        </>
      )}
    </div>
  </div>
);
