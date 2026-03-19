import { NotableContributionCard } from "@/components/notable-contribution-card";

import type { NotableContribution } from "@ps/api/gen/prism/v1/insights_pb";

export const OrgHighlights = ({
  highlights,
}: {
  highlights: NotableContribution[];
}): React.ReactElement | null => {
  if (highlights.length === 0) return null;

  return (
    <div className="space-y-3">
      <h2 className="text-sm font-medium">Highlights</h2>
      <div className="grid gap-3 lg:grid-cols-2">
        {highlights.slice(0, 4).map((item) => (
          <NotableContributionCard key={item.contributionId} item={item} />
        ))}
      </div>
    </div>
  );
};
