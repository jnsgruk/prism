import { Badge } from "@/components/ui/badge";
import { cn } from "@ps/cn";

import type { TopicCategoryCount } from "@ps/api/gen/canonical/prism/v1/insights_pb";

export const CategoryTags = ({
  categories,
  className,
}: {
  categories: TopicCategoryCount[];
  className?: string;
}): React.ReactElement | null => {
  if (categories.length === 0) return null;

  return (
    <div className={cn("flex flex-wrap gap-1.5", className)}>
      {categories.map((c) => (
        <Badge key={c.category} variant="outline" className="gap-1 text-xs">
          {c.category}
          <span className="tabular-nums text-muted-foreground">({c.count})</span>
        </Badge>
      ))}
    </div>
  );
};
