import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { fmtHours } from "@/lib/format-metrics";
import { ArrowRight, Gauge, Info } from "lucide-react";

import type { TeamMetrics } from "@ps/api/gen/canonical/prism/v1/metrics_pb";

const MetricValue = ({
  value,
  label,
  description,
}: {
  value: string;
  label: string;
  description: string;
}): React.ReactElement => (
  <div className="min-w-0 flex-1">
    <p className="text-2xl font-semibold tabular-nums">{value}</p>
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
  </div>
);

const buildSummary = (metrics: TeamMetrics): string => {
  const parts: string[] = [];
  if (metrics.avgCycleTimeHours > 0) {
    parts.push(`Average ${fmtHours(metrics.avgCycleTimeHours)} from first commit to merge`);
  }
  if (metrics.wipAvg > 0) {
    parts.push(`${metrics.wipAvg.toFixed(1)} PRs open at any given time`);
  }
  if (metrics.leadTimeHours > 0) {
    parts.push(`${fmtHours(metrics.leadTimeHours)} lead time from issue to merge`);
  }
  if (parts.length === 0) return "No flow data available for this period.";
  return parts.join(". ") + ".";
};

export const FlowPanel = ({
  metrics,
}: {
  metrics: TeamMetrics | undefined;
}): React.ReactElement | null => {
  if (!metrics) return null;

  const cycleTime = metrics.avgCycleTimeHours;
  const wip = metrics.wipAvg;
  const leadTime = metrics.leadTimeHours;
  const flowEfficiency = metrics.flowEfficiency;
  const hasFlowMetrics = cycleTime > 0 || wip > 0 || leadTime > 0 || flowEfficiency > 0;

  if (!hasFlowMetrics) return null;

  return (
    <TooltipProvider>
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <Gauge className="size-4 text-muted-foreground" />
            <CardTitle>Flow</CardTitle>
          </div>
          <CardDescription>DORA-style delivery pipeline metrics.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
            <MetricValue
              value={fmtHours(cycleTime)}
              label="Cycle Time"
              description="Average time from first commit to PR merge."
            />
            <MetricValue
              value={wip > 0 ? wip.toFixed(1) : "\u2014"}
              label="WIP"
              description="Average number of open PRs (work in progress) during the period."
            />
            <MetricValue
              value={fmtHours(leadTime)}
              label="Lead Time"
              description="Average time from issue creation to PR merge."
            />
            <MetricValue
              value={flowEfficiency > 0 ? `${Math.round(flowEfficiency * 100)}%` : "\u2014"}
              label="Flow Efficiency"
              description="Ratio of active work time to total lead time. Higher is better."
            />
          </div>

          <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
            <ArrowRight className="size-3.5 shrink-0" />
            {buildSummary(metrics)}
          </p>
        </CardContent>
      </Card>
    </TooltipProvider>
  );
};
