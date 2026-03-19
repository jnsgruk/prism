import type { TypeCoverage } from "@ps/api/gen/prism/v1/insights_pb";
import { cn } from "@ps/cn";

const typeLabel = (type: string): string => {
  switch (type) {
    case "review_depth":
      return "Review depth";
    case "sentiment":
      return "Sentiment";
    case "significance":
      return "PR significance";
    case "topic":
      return "Topic classification";
    default:
      return type;
  }
};

export const CoverageIndicator = ({
  byType,
  className,
}: {
  byType: TypeCoverage[];
  className?: string;
}): React.ReactElement | null => {
  if (byType.length === 0) return null;

  return (
    <div className={cn("space-y-1.5", className)}>
      {byType.map((t) => {
        const pct = t.eligible > 0 ? (t.enriched / t.eligible) * 100 : 0;
        return (
          <div key={t.enrichmentType} className="flex items-center gap-2 text-xs">
            <span className="w-28 shrink-0 text-muted-foreground">
              {typeLabel(t.enrichmentType)}
            </span>
            <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
              <div
                className="h-full rounded-full bg-primary"
                style={{ width: `${Math.min(pct, 100)}%` }}
              />
            </div>
            <span className="w-16 shrink-0 tabular-nums text-muted-foreground text-right">
              {t.enriched}/{t.eligible}
            </span>
          </div>
        );
      })}
    </div>
  );
};
