import { Card, CardContent } from "@/components/ui/card";
import { GitPullRequest, Clock, Users, Activity } from "lucide-react";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";

const MetricCard = ({
  label,
  value,
  icon: Icon,
}: {
  label: string;
  value: string;
  icon: React.ComponentType<{ className?: string }>;
}): React.ReactElement => (
  <Card>
    <CardContent className="flex items-center gap-3 p-4">
      <div className="rounded-md bg-muted p-2">
        <Icon className="size-4 text-muted-foreground" />
      </div>
      <div>
        <p className="text-2xl font-semibold leading-none">{value}</p>
        <p className="mt-1 text-xs text-muted-foreground">{label}</p>
      </div>
    </CardContent>
  </Card>
);

export const TeamMetricCards = ({
  metrics,
  memberCount,
}: {
  metrics: TeamMetrics | undefined;
  memberCount: number;
}): React.ReactElement => {
  const throughput = metrics?.throughput ?? 0;
  const avgReview = metrics?.avgReviewTurnaroundHours ?? 0;

  return (
    <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
      <MetricCard icon={GitPullRequest} label="Merged PRs" value={String(throughput)} />
      <MetricCard
        icon={Clock}
        label="Avg Review (hrs)"
        value={avgReview > 0 ? avgReview.toFixed(1) : "\u2014"}
      />
      <MetricCard icon={Users} label="Members" value={String(memberCount)} />
      <MetricCard
        icon={Activity}
        label="Active Contributors"
        value={metrics ? String(metrics.memberCount) : "\u2014"}
      />
    </div>
  );
};
