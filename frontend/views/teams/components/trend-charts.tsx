import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Bar,
  BarChart,
  CartesianGrid,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import type { GetFlowMetricsResponse } from "@ps/api/gen/prism/v1/metrics_pb";
import type { TooltipContentProps } from "recharts/types/component/Tooltip";

const ChartTooltip = ({
  active,
  payload,
  label,
}: TooltipContentProps): React.ReactElement | null => {
  if (!active || !payload?.length) return null;
  return (
    <div className="rounded-md border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-md">
      <p className="mb-1 font-medium">{label}</p>
      {payload.map((entry) => (
        <p key={entry.name} className="text-muted-foreground">
          {entry.name}: {entry.value}
        </p>
      ))}
    </div>
  );
};

const cursorStyle = { fill: "hsl(var(--muted))", opacity: 0.5 };

export const ThroughputTrendChart = ({
  flowMetrics,
}: {
  flowMetrics: GetFlowMetricsResponse | undefined;
}): React.ReactElement | null => {
  const data = (flowMetrics?.throughputTrend ?? []).map((t) => ({
    date: t.date,
    count: t.count,
  }));

  if (data.length <= 1) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Throughput Trend</CardTitle>
        <CardDescription>
          Merged pull requests per period, showing delivery pace over time.
        </CardDescription>
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={250}>
          <BarChart data={data} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
            <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
            <XAxis dataKey="date" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
            <YAxis className="fill-muted-foreground" />
            <Tooltip content={ChartTooltip} cursor={cursorStyle} />
            <Bar
              dataKey="count"
              name="Completed items"
              fill="hsl(var(--primary))"
              radius={[4, 4, 0, 0]}
            />
          </BarChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  );
};

export const WipTrendChart = ({
  flowMetrics,
}: {
  flowMetrics: GetFlowMetricsResponse | undefined;
}): React.ReactElement | null => {
  const data = (flowMetrics?.wipTrend ?? []).map((w) => ({
    date: w.date,
    wip: Math.round(w.wip * 10) / 10,
  }));

  if (data.length <= 1) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle>WIP Trend</CardTitle>
        <CardDescription>Average open pull requests (work in progress) per period.</CardDescription>
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={250}>
          <LineChart data={data} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
            <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
            <XAxis dataKey="date" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
            <YAxis className="fill-muted-foreground" />
            <Tooltip content={ChartTooltip} cursor={cursorStyle} />
            <Line
              type="monotone"
              dataKey="wip"
              name="WIP"
              stroke="hsl(var(--primary))"
              strokeWidth={2}
              dot={{ fill: "hsl(var(--primary))", r: 3 }}
            />
          </LineChart>
        </ResponsiveContainer>
      </CardContent>
    </Card>
  );
};
