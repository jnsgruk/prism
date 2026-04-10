import { ChartTooltip, cursorStyle } from "@/components/chart-tooltip";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { fmtFloat } from "@/lib/format-metrics";
import type { GetIndividualProfileResponse } from "@/lib/hooks/use-metrics";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";

export const ActivityChart = ({ profile }: { profile: GetIndividualProfileResponse }): React.ReactElement | null => {
  if (profile.activityByPlatform.length === 0) return null;

  const data = profile.activityByPlatform.map((a) => ({
    platform: a.platform,
    count: a.contributionCount,
  }));

  return (
    <Card>
      <CardHeader>
        <CardTitle>Activity by platform</CardTitle>
        <CardDescription>Contribution counts per platform in this period</CardDescription>
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={250}>
          <BarChart data={data} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
            <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
            <XAxis dataKey="platform" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
            <YAxis className="fill-muted-foreground" allowDecimals={false} />
            <Tooltip content={ChartTooltip} cursor={cursorStyle} />
            <Bar dataKey="count" name="Contributions" fill="hsl(var(--primary))" radius={[4, 4, 0, 0]} />
          </BarChart>
        </ResponsiveContainer>
        {/* Per-platform key metrics */}
        <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {profile.activityByPlatform.map((a) => (
            <div key={a.platform} className="rounded-md border px-3 py-2">
              <p className="text-sm font-medium">{a.platform}</p>
              <p className="text-xs text-muted-foreground">
                {a.contributionCount} contribution{a.contributionCount !== 1 ? "s" : ""}
                {a.metrics["avg_review_hours"] != null &&
                  ` \u00b7 avg review ${fmtFloat(a.metrics["avg_review_hours"])}h`}
                {a.metrics["avg_cycle_time_hours"] != null &&
                  ` \u00b7 avg cycle ${fmtFloat(a.metrics["avg_cycle_time_hours"])}h`}
              </p>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
};
