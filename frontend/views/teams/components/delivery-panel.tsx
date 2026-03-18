import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { ChartTooltip, cursorStyle } from "@/components/chart-tooltip";
import { fmtHours } from "@/lib/format-metrics";
import { ArrowRight, GitPullRequest, Info } from "lucide-react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip as RechartsTooltip,
  XAxis,
  YAxis,
} from "recharts";

import type { GetFlowMetricsResponse, TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";

const MetricValue = ({
  value,
  label,
  description,
  secondary,
  onClick,
}: {
  value: string;
  label: string;
  description: string;
  secondary?: string;
  onClick?: () => void;
}): React.ReactElement => (
  <div className="min-w-0 flex-1">
    <button
      type="button"
      onClick={onClick}
      disabled={!onClick}
      className="group text-left disabled:cursor-default"
    >
      <span className="text-2xl font-semibold tabular-nums group-enabled:underline-offset-4 group-enabled:hover:underline">
        {value}
      </span>
    </button>
    <div className="mt-0.5 flex items-center gap-1">
      <span className="text-xs text-muted-foreground">{label}</span>
      <Tooltip>
        <TooltipTrigger render={<button type="button" className="inline-flex shrink-0" />}>
          <Info className="size-3 text-muted-foreground/50" />
        </TooltipTrigger>
        <TooltipContent side="bottom" className="max-w-64">
          {description}
        </TooltipContent>
      </Tooltip>
    </div>
    {secondary && <p className="mt-0.5 text-[10px] text-muted-foreground/70">{secondary}</p>}
  </div>
);

const buildSummary = (metrics: TeamMetrics, memberCount: number): string => {
  const parts: string[] = [];
  if (metrics.throughput > 0) {
    parts.push(
      `${metrics.throughput} merged pull request${metrics.throughput !== 1 ? "s" : ""} from ${metrics.memberCount} contributor${metrics.memberCount !== 1 ? "s" : ""}`,
    );
  }
  if (metrics.reviewTurnaroundP75Hours > 0) {
    parts.push(`75% of reviews completed within ${fmtHours(metrics.reviewTurnaroundP75Hours)}`);
  }
  if (parts.length === 0) {
    return `${memberCount} team member${memberCount !== 1 ? "s" : ""}, no merged PRs in this period.`;
  }
  return parts.join(". ") + ".";
};

export const DeliveryPanel = ({
  metrics,
  memberCount,
  flowMetrics,
  onScrollToPrs,
  onScrollToReviews,
  onScrollToMembers,
}: {
  metrics: TeamMetrics | undefined;
  memberCount: number;
  flowMetrics: GetFlowMetricsResponse | undefined;
  onScrollToPrs?: () => void;
  onScrollToReviews?: () => void;
  onScrollToMembers?: () => void;
}): React.ReactElement | null => {
  if (!metrics) return null;

  const throughput = metrics.throughput;
  const p75 = metrics.reviewTurnaroundP75Hours;
  const p90 = metrics.reviewTurnaroundP90Hours;
  const p99 = metrics.reviewTurnaroundP99Hours;
  const activeContributors = metrics.memberCount;

  const trendData = (flowMetrics?.throughputTrend ?? []).map((t) => ({
    date: t.date,
    count: t.count,
  }));
  const showTrend = trendData.length > 1;

  return (
    <TooltipProvider>
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <GitPullRequest className="size-4 text-muted-foreground" />
            <CardTitle>Delivery</CardTitle>
          </div>
          <CardDescription>Pull request throughput and review speed.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* Metric values row */}
          <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
            <MetricValue
              value={String(throughput)}
              label="Merged PRs"
              description="Total merged pull requests in the selected period."
              onClick={onScrollToPrs}
            />
            <MetricValue
              value={fmtHours(p75)}
              label="Review P75"
              description="Time from PR ready-for-review to first review, 75th percentile."
              secondary={p75 > 0 ? `P90 ${fmtHours(p90)} · P99 ${fmtHours(p99)}` : undefined}
              onClick={onScrollToReviews}
            />
            <MetricValue
              value={`${activeContributors} / ${memberCount}`}
              label="Active / Members"
              description="Members with at least one merged PR or review in the period, out of total team members."
              onClick={onScrollToMembers}
            />
          </div>

          {/* Throughput trend (moved from orphan chart) */}
          {showTrend && (
            <div>
              <h4 className="mb-2 text-sm font-medium text-muted-foreground">Throughput Trend</h4>
              <ResponsiveContainer width="100%" height={180}>
                <BarChart data={trendData} margin={{ top: 5, right: 10, left: 0, bottom: 5 }}>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                  <XAxis dataKey="date" tick={{ fontSize: 11 }} className="fill-muted-foreground" />
                  <YAxis allowDecimals={false} className="fill-muted-foreground" />
                  <RechartsTooltip content={ChartTooltip} cursor={cursorStyle} />
                  <Bar
                    dataKey="count"
                    name="Merged PRs"
                    fill="hsl(var(--primary))"
                    radius={[4, 4, 0, 0]}
                  />
                </BarChart>
              </ResponsiveContainer>
            </div>
          )}

          {/* Plain-English summary */}
          <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
            <ArrowRight className="size-3.5 shrink-0" />
            {buildSummary(metrics, memberCount)}
          </p>
        </CardContent>
      </Card>
    </TooltipProvider>
  );
};
