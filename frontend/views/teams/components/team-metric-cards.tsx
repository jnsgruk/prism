import { Card, CardContent } from "@/components/ui/card";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import {
  GitPullRequest,
  Clock,
  Users,
  Activity,
  Timer,
  Layers,
  ArrowRight,
  Gauge,
  Info,
  MessageSquarePlus,
  MessagesSquare,
  ThumbsUp,
} from "lucide-react";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";
import { fmtHours } from "@/lib/format-metrics";

const MetricCard = ({
  label,
  value,
  icon: Icon,
  secondary,
  description,
}: {
  label: string;
  value: string;
  icon: React.ComponentType<{ className?: string }>;
  secondary?: string;
  description?: string;
}): React.ReactElement => (
  <Card>
    <CardContent className="flex items-center gap-3 p-4">
      <div className="rounded-md bg-muted p-2">
        <Icon className="size-4 text-muted-foreground" />
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-2xl font-semibold leading-none">{value}</p>
        <div className="mt-1 flex items-center gap-1">
          <p className="text-xs text-muted-foreground">{label}</p>
          {description && (
            <Tooltip>
              <TooltipTrigger render={<button type="button" className="inline-flex shrink-0" />}>
                <Info className="size-3 text-muted-foreground/50" />
              </TooltipTrigger>
              <TooltipContent side="bottom" className="max-w-64">
                {description}
              </TooltipContent>
            </Tooltip>
          )}
        </div>
        {secondary && <p className="mt-0.5 text-[10px] text-muted-foreground/70">{secondary}</p>}
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
  const p75 = metrics?.reviewTurnaroundP75Hours ?? 0;
  const p90 = metrics?.reviewTurnaroundP90Hours ?? 0;
  const p99 = metrics?.reviewTurnaroundP99Hours ?? 0;
  const cycleTime = metrics?.avgCycleTimeHours ?? 0;
  const wip = metrics?.wipAvg ?? 0;
  const leadTime = metrics?.leadTimeHours ?? 0;
  const flowEfficiency = metrics?.flowEfficiency ?? 0;
  const hasFlowMetrics = cycleTime > 0 || wip > 0 || leadTime > 0 || flowEfficiency > 0;

  const discourseTopics = metrics?.discourseTopicsCreated ?? 0;
  const discoursePosts = metrics?.discoursePosts ?? 0;
  const discourseReplies = metrics?.discourseReplies ?? 0;
  const discourseLikesGiven = metrics?.discourseLikesGiven ?? 0;
  const hasDiscourse = discourseTopics > 0 || discoursePosts > 0;

  // Build per-instance secondary text
  const instanceBreakdown = (
    field: "topicsCreated" | "posts" | "likesGiven",
  ): string | undefined => {
    const instances = metrics?.discourseByInstance ?? [];
    if (instances.length <= 1) return undefined;
    return instances
      .filter((i) => i[field] > 0)
      .map((i) => `${i.instance}: ${i[field]}`)
      .join(" \u00b7 ");
  };

  return (
    <TooltipProvider>
      <div className="space-y-4">
        <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
          <MetricCard
            icon={GitPullRequest}
            label="Throughput"
            value={String(throughput)}
            description="Total merged pull requests in the selected period."
          />
          <MetricCard
            icon={Clock}
            label="Review Turnaround (P75)"
            value={fmtHours(p75)}
            secondary={p75 > 0 ? `P90 ${fmtHours(p90)} · P99 ${fmtHours(p99)}` : undefined}
            description="Time from PR ready-for-review to first review, 75th percentile."
          />
          <MetricCard
            icon={Users}
            label="Members"
            value={String(memberCount)}
            description="Total people assigned to this team, including members of child teams."
          />
          <MetricCard
            icon={Activity}
            label="Active Contributors"
            value={metrics ? String(metrics.memberCount) : "\u2014"}
            description="Members with at least one merged PR or review in the selected period."
          />
        </div>
        {hasFlowMetrics && (
          <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
            <MetricCard
              icon={Timer}
              label="Avg Cycle Time"
              value={fmtHours(cycleTime)}
              description="Average time from first commit to PR merge."
            />
            <MetricCard
              icon={Layers}
              label="WIP"
              value={wip > 0 ? wip.toFixed(1) : "\u2014"}
              description="Average number of open PRs (work in progress) during the period."
            />
            <MetricCard
              icon={ArrowRight}
              label="Lead Time"
              value={fmtHours(leadTime)}
              description="Average time from issue creation to PR merge."
            />
            <MetricCard
              icon={Gauge}
              label="Flow Efficiency"
              value={flowEfficiency > 0 ? `${Math.round(flowEfficiency * 100)}%` : "\u2014"}
              description="Ratio of active work time to total lead time. Higher is better."
            />
          </div>
        )}
        {hasDiscourse && (
          <div className="grid grid-cols-3 gap-4">
            <MetricCard
              icon={MessageSquarePlus}
              label="Topics Created"
              value={String(discourseTopics)}
              secondary={instanceBreakdown("topicsCreated")}
              description="New Discourse topics started by team members."
            />
            <MetricCard
              icon={MessagesSquare}
              label="Posts & Replies"
              value={String(discoursePosts)}
              secondary={discourseReplies > 0 ? `${discourseReplies} replies` : undefined}
              description="Total posts on Discourse, including replies."
            />
            <MetricCard
              icon={ThumbsUp}
              label="Likes Given"
              value={String(discourseLikesGiven)}
              secondary={instanceBreakdown("likesGiven")}
              description="Likes given by team members on Discourse."
            />
          </div>
        )}
      </div>
    </TooltipProvider>
  );
};
