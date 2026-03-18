import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { ChartTooltip, cursorStyle } from "@/components/chart-tooltip";
import { fmtHours } from "@/lib/format-metrics";
import { ArrowRight, GitPullRequest, Info } from "lucide-react";
import { useMemo } from "react";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Legend,
  ResponsiveContainer,
  Tooltip as RechartsTooltip,
  XAxis,
  YAxis,
} from "recharts";

import type { GetFlowMetricsResponse, TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";

/** Capitalise a source key like "github" → "GitHub", "discourse" → "Discourse", "jira" → "Jira". */
const sourceLabel = (key: string): string => {
  if (key === "github") return "GitHub";
  if (key === "discourse") return "Discourse";
  return key.charAt(0).toUpperCase() + key.slice(1);
};

/** Stable distinct colour palette for stacked bars. */
const SOURCE_COLORS = [
  "hsl(221 83% 53%)", // blue
  "hsl(142 71% 45%)", // green
  "hsl(38 92% 50%)", // amber
  "hsl(262 83% 58%)", // purple
  "hsl(0 84% 60%)", // red
];

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
      `${metrics.throughput} completed item${metrics.throughput !== 1 ? "s" : ""} from ${metrics.memberCount} contributor${metrics.memberCount !== 1 ? "s" : ""}`,
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

  // Build stacked trend data: each data point has { date, <source1>: n, <source2>: n, ... }
  // Discourse instances (discourse-ubuntu, discourse-ask, etc.) are grouped into a single "discourse" bucket.
  const throughputTrend = flowMetrics?.throughputTrend;
  const { trendData, sourceKeys } = useMemo(() => {
    const points = throughputTrend ?? [];

    // Group per-source counts, merging all discourse-* into "discourse"
    const groupKey = (key: string): string => (key.startsWith("discourse") ? "discourse" : key);

    const allKeys = new Set<string>();
    for (const t of points) {
      for (const key of Object.keys(t.bySource)) {
        allKeys.add(groupKey(key));
      }
    }
    const keys = [...allKeys].toSorted();

    const data = points.map((t) => {
      const point: Record<string, string | number> = { date: t.date };
      if (keys.length === 0) {
        point["total"] = t.count;
      } else {
        // Initialise all keys to 0, then accumulate
        for (const key of keys) {
          point[key] = 0;
        }
        for (const [rawKey, count] of Object.entries(t.bySource)) {
          const grouped = groupKey(rawKey);
          point[grouped] = (point[grouped] as number) + count;
        }
      }
      return point;
    });

    return { trendData: data, sourceKeys: keys };
  }, [throughputTrend]);

  const showTrend = trendData.length > 1;
  const hasMultipleSources = sourceKeys.length > 1;

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
              label="Throughput"
              description="Total completed items in the period: merged PRs, resolved Jira tickets, and Discourse topics."
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

          {/* Throughput trend — stacked by source when breakdown is available */}
          {showTrend && (
            <div>
              <h4 className="mb-2 text-sm font-medium text-muted-foreground">Throughput Trend</h4>
              <p className="mb-2 text-xs text-muted-foreground/70">
                Completed items per period (merged PRs, resolved tickets, topics).
              </p>
              <ResponsiveContainer width="100%" height={hasMultipleSources ? 210 : 180}>
                <BarChart data={trendData} margin={{ top: 5, right: 10, left: 0, bottom: 5 }}>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                  <XAxis dataKey="date" tick={{ fontSize: 11 }} className="fill-muted-foreground" />
                  <YAxis allowDecimals={false} className="fill-muted-foreground" />
                  <RechartsTooltip content={ChartTooltip} cursor={cursorStyle} />
                  {sourceKeys.length > 0 ? (
                    sourceKeys.map((key, i) => (
                      <Bar
                        key={key}
                        dataKey={key}
                        name={sourceLabel(key)}
                        stackId="throughput"
                        fill={SOURCE_COLORS[i % SOURCE_COLORS.length]!}
                        radius={i === sourceKeys.length - 1 ? [4, 4, 0, 0] : [0, 0, 0, 0]}
                      />
                    ))
                  ) : (
                    <Bar
                      dataKey="total"
                      name="Completed items"
                      fill="hsl(var(--primary))"
                      radius={[4, 4, 0, 0]}
                    />
                  )}
                  {hasMultipleSources && (
                    <Legend iconType="square" iconSize={10} wrapperStyle={{ fontSize: 11 }} />
                  )}
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
