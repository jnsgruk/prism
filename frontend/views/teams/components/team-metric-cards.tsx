import { Card, CardContent } from "@/components/ui/card";
import { GitPullRequest, Clock, Users, Activity } from "lucide-react";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";
export type DrilldownMetric = "throughput" | "review_turnaround" | null;

const MetricCard = ({
  label,
  value,
  icon: Icon,
  secondary,
  onClick,
}: {
  label: string;
  value: string;
  icon: React.ComponentType<{ className?: string }>;
  secondary?: string;
  onClick?: () => void;
}): React.ReactElement => (
  <Card className={onClick ? "cursor-pointer transition-colors hover:bg-muted/50" : undefined}>
    <CardContent
      className="flex items-center gap-3 p-4"
      onClick={onClick}
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
      onKeyDown={
        onClick
          ? (e): void => {
              if (e.key === "Enter") onClick();
            }
          : undefined
      }
    >
      <div className="rounded-md bg-muted p-2">
        <Icon className="size-4 text-muted-foreground" />
      </div>
      <div>
        <p className="text-2xl font-semibold leading-none">{value}</p>
        <p className="mt-1 text-xs text-muted-foreground">{label}</p>
        {secondary && <p className="mt-0.5 text-[10px] text-muted-foreground/70">{secondary}</p>}
      </div>
    </CardContent>
  </Card>
);

const formatHours = (h: number): string => (h > 0 ? `${h.toFixed(1)}h` : "\u2014");

export const TeamMetricCards = ({
  metrics,
  memberCount,
  onDrillDown,
}: {
  metrics: TeamMetrics | undefined;
  memberCount: number;
  onDrillDown?: (metric: DrilldownMetric) => void;
}): React.ReactElement => {
  const throughput = metrics?.throughput ?? 0;
  const p75 = metrics?.reviewTurnaroundP75Hours ?? 0;
  const p90 = metrics?.reviewTurnaroundP90Hours ?? 0;
  const p99 = metrics?.reviewTurnaroundP99Hours ?? 0;

  return (
    <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
      <MetricCard
        icon={GitPullRequest}
        label="Merged PRs"
        value={String(throughput)}
        onClick={throughput > 0 && onDrillDown ? () => onDrillDown("throughput") : undefined}
      />
      <MetricCard
        icon={Clock}
        label="Review Turnaround (P75)"
        value={formatHours(p75)}
        secondary={p75 > 0 ? `P90 ${formatHours(p90)} · P99 ${formatHours(p99)}` : undefined}
        onClick={p75 > 0 && onDrillDown ? () => onDrillDown("review_turnaround") : undefined}
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
