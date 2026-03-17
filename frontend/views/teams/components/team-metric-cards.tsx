import { Card, CardContent } from "@/components/ui/card";
import {
  GitPullRequest,
  Clock,
  Users,
  Activity,
  Timer,
  Layers,
  ArrowRight,
  Gauge,
} from "lucide-react";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";

const MetricCard = ({
  label,
  value,
  icon: Icon,
  secondary,
}: {
  label: string;
  value: string;
  icon: React.ComponentType<{ className?: string }>;
  secondary?: string;
}): React.ReactElement => (
  <Card>
    <CardContent className="flex items-center gap-3 p-4">
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
}: {
  metrics: TeamMetrics | undefined;
  memberCount: number;
}): React.ReactElement => {
  const throughput = metrics?.throughput ?? 0;
  const p75 = metrics?.reviewTurnaroundP75Hours ?? 0;
  const p90 = metrics?.reviewTurnaroundP90Hours ?? 0;
  const p99 = metrics?.reviewTurnaroundP99Hours ?? 0;
  const cycleTime = metrics?.avgCycleTimeHours ?? 0;
  const wip = metrics?.wipAvg ?? 0;
  const leadTime = metrics?.leadTimeHours ?? 0;
  const flowEfficiency = metrics?.flowEfficiency ?? 0;
  const hasFlowMetrics = cycleTime > 0 || wip > 0 || leadTime > 0 || flowEfficiency > 0;

  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
        <MetricCard icon={GitPullRequest} label="Throughput" value={String(throughput)} />
        <MetricCard
          icon={Clock}
          label="Review Turnaround (P75)"
          value={formatHours(p75)}
          secondary={p75 > 0 ? `P90 ${formatHours(p90)} · P99 ${formatHours(p99)}` : undefined}
        />
        <MetricCard icon={Users} label="Members" value={String(memberCount)} />
        <MetricCard
          icon={Activity}
          label="Active Contributors"
          value={metrics ? String(metrics.memberCount) : "\u2014"}
        />
      </div>
      {hasFlowMetrics && (
        <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
          <MetricCard icon={Timer} label="Avg Cycle Time" value={formatHours(cycleTime)} />
          <MetricCard icon={Layers} label="WIP" value={wip > 0 ? wip.toFixed(1) : "\u2014"} />
          <MetricCard icon={ArrowRight} label="Lead Time" value={formatHours(leadTime)} />
          <MetricCard
            icon={Gauge}
            label="Flow Efficiency"
            value={flowEfficiency > 0 ? `${Math.round(flowEfficiency * 100)}%` : "\u2014"}
          />
        </div>
      )}
    </div>
  );
};
