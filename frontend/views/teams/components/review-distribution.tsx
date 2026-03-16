import { useMemo } from "react";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";

import type { Period } from "@ps/api/gen/prism/v1/metrics_pb";
import type { ContributionFilters } from "@/lib/hooks/use-metrics";
import { useListTeamContributions } from "@/lib/hooks/use-metrics";

const BUCKETS = [
  { label: "< 1h", max: 1 },
  { label: "1-4h", max: 4 },
  { label: "4-8h", max: 8 },
  { label: "8-24h", max: 24 },
  { label: "24-72h", max: 72 },
  { label: "72h+", max: Infinity },
] as const;

export const ReviewDistribution = ({
  teamId,
  period,
}: {
  teamId: string;
  period: Period;
}): React.ReactElement | null => {
  // Fetch all reviews (large page to get distribution data)
  const filters: ContributionFilters = {
    contributionType: "pr_review",
    pageSize: 500,
    pageIndex: 0,
  };

  const { data } = useListTeamContributions(teamId, period, filters);

  const chartData = useMemo(() => {
    const reviews = data?.contributions ?? [];
    const hoursValues = reviews.map((r) => r.reviewHours).filter((h) => h > 0);

    if (hoursValues.length === 0) return null;

    return BUCKETS.map((bucket, i) => {
      const min = i === 0 ? 0 : BUCKETS[i - 1]!.max;
      const count = hoursValues.filter((h) => h >= min && h < bucket.max).length;
      return { name: bucket.label, count };
    });
  }, [data]);

  if (!chartData) return null;

  return (
    <div className="mb-4 space-y-2">
      <h4 className="text-sm font-medium">Turnaround Distribution</h4>
      <ResponsiveContainer width="100%" height={160}>
        <BarChart data={chartData} margin={{ top: 5, right: 10, left: 0, bottom: 5 }}>
          <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
          <XAxis dataKey="name" tick={{ fontSize: 11 }} className="fill-muted-foreground" />
          <YAxis allowDecimals={false} tick={{ fontSize: 11 }} className="fill-muted-foreground" />
          <Tooltip
            contentStyle={{
              backgroundColor: "hsl(var(--popover))",
              border: "1px solid hsl(var(--border))",
              borderRadius: "var(--radius)",
              color: "hsl(var(--popover-foreground))",
            }}
          />
          <Bar dataKey="count" name="Reviews" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} />
        </BarChart>
      </ResponsiveContainer>
    </div>
  );
};
