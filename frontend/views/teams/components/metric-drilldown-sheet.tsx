import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetDescription,
} from "@/components/ui/sheet";

import type { Period } from "@ps/api/gen/prism/v1/metrics_pb";
import { ContributionTable } from "@/views/teams/components/contribution-table";
import { ReviewDistribution } from "@/views/teams/components/review-distribution";

export type DrilldownMetric = "throughput" | "review_turnaround" | null;

export interface DrilldownTarget {
  metric: DrilldownMetric;
  teamId: string;
  teamName: string;
}

const metricConfig: Record<
  NonNullable<DrilldownMetric>,
  { title: string; description: string; contributionType?: string; state?: string }
> = {
  throughput: {
    title: "Merged PRs",
    description: "Pull requests merged during this period",
    contributionType: "pull_request",
    state: "merged",
  },
  review_turnaround: {
    title: "Review Turnaround",
    description: "PR reviews sorted by turnaround time",
    contributionType: "pr_review",
  },
};

export const MetricDrilldownSheet = ({
  target,
  period,
  onClose,
}: {
  target: DrilldownTarget | null;
  period: Period;
  onClose: () => void;
}): React.ReactElement => {
  const open = target !== null && target.metric !== null;
  const config = target?.metric ? metricConfig[target.metric] : null;

  return (
    <Sheet
      open={open}
      onOpenChange={(isOpen) => {
        if (!isOpen) onClose();
      }}
    >
      <SheetContent side="right" className="sm:max-w-2xl">
        {config && target && (
          <>
            <SheetHeader>
              <SheetTitle>
                {config.title} — {target.teamName}
              </SheetTitle>
              <SheetDescription>{config.description}</SheetDescription>
            </SheetHeader>
            <div className="min-h-0 flex-1 overflow-y-auto px-4 pb-4">
              {target.metric === "review_turnaround" && (
                <ReviewDistribution teamId={target.teamId} period={period} />
              )}
              <ContributionTable
                teamId={target.teamId}
                period={period}
                defaultContributionType={config.contributionType}
                defaultState={config.state}
              />
            </div>
          </>
        )}
      </SheetContent>
    </Sheet>
  );
};
