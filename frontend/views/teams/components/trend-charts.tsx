import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
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

const tooltipStyle = {
  backgroundColor: "hsl(var(--popover))",
  border: "1px solid hsl(var(--border))",
  borderRadius: "var(--radius)",
  color: "hsl(var(--popover-foreground))",
};

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
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={250}>
          <BarChart data={data} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
            <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
            <XAxis dataKey="date" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
            <YAxis className="fill-muted-foreground" />
            <Tooltip contentStyle={tooltipStyle} />
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
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={250}>
          <LineChart data={data} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
            <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
            <XAxis dataKey="date" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
            <YAxis className="fill-muted-foreground" />
            <Tooltip contentStyle={tooltipStyle} />
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
