import { Card, CardContent } from "@/components/ui/card";
import { Table, TableBody, TableHeader, TableRow } from "@/components/ui/table";
import { useMemo, useState } from "react";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";
import type { Team } from "@ps/api/gen/prism/v1/org_pb";
import { ChartTooltip, cursorStyle } from "@/components/chart-tooltip";
import { MetricsRow } from "@/views/teams/components/metrics-row";
import { SortableHeader } from "@/views/teams/components/sortable-header";
import type { SortDir, SortField } from "@/views/teams/components/sortable-header";

export const ComparisonTable = ({
  childMetrics,
  selectedTeam,
  sourcePlatforms = [],
}: {
  childMetrics: TeamMetrics[];
  selectedTeam: Team;
  sourcePlatforms?: string[];
}): React.ReactElement => {
  const [sortField, setSortField] = useState<SortField>("throughput");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  const hasDiscourse = sourcePlatforms.some((p) => p.startsWith("discourse-"));

  const toggleSort = (field: SortField): void => {
    if (sortField === field) {
      setSortDir(sortDir === "asc" ? "desc" : "asc");
    } else {
      setSortField(field);
      setSortDir("desc");
    }
  };

  const sortedMetrics = useMemo(() => {
    const sorted = [...childMetrics];
    const dir = sortDir === "asc" ? 1 : -1;
    sorted.sort((a, b) => {
      switch (sortField) {
        case "name":
          return dir * a.teamName.localeCompare(b.teamName);
        case "throughput":
          return dir * (a.throughput - b.throughput);
        case "reviewP75":
          return dir * (a.reviewTurnaroundP75Hours - b.reviewTurnaroundP75Hours);
        case "members":
          return dir * (a.memberCount - b.memberCount);
        case "cycleTime":
          return dir * (a.avgCycleTimeHours - b.avgCycleTimeHours);
        case "discourseTopics":
          return dir * (a.discourseTopicsCreated - b.discourseTopicsCreated);
        case "discoursePosts":
          return dir * (a.discoursePosts - b.discoursePosts);
        default:
          return 0;
      }
    });
    return sorted;
  }, [childMetrics, sortField, sortDir]);

  const chartData = useMemo(
    () =>
      sortedMetrics.map((m) => ({
        name: m.teamName,
        throughput: m.throughput,
        reviewP75Hours: Math.round(m.reviewTurnaroundP75Hours * 10) / 10,
      })),
    [sortedMetrics],
  );

  return (
    <>
      <Card>
        <CardContent className="overflow-x-auto p-0">
          <Table>
            <TableHeader>
              <TableRow>
                <SortableHeader field="name" current={sortField} dir={sortDir} onSort={toggleSort}>
                  Team
                </SortableHeader>
                <SortableHeader
                  field="throughput"
                  current={sortField}
                  dir={sortDir}
                  onSort={toggleSort}
                >
                  Throughput
                </SortableHeader>
                <SortableHeader
                  field="reviewP75"
                  current={sortField}
                  dir={sortDir}
                  onSort={toggleSort}
                >
                  Review P75
                </SortableHeader>
                <SortableHeader
                  field="cycleTime"
                  current={sortField}
                  dir={sortDir}
                  onSort={toggleSort}
                >
                  Cycle Time
                </SortableHeader>
                {hasDiscourse && (
                  <SortableHeader
                    field="discourseTopics"
                    current={sortField}
                    dir={sortDir}
                    onSort={toggleSort}
                  >
                    Topics
                  </SortableHeader>
                )}
                {hasDiscourse && (
                  <SortableHeader
                    field="discoursePosts"
                    current={sortField}
                    dir={sortDir}
                    onSort={toggleSort}
                  >
                    Posts
                  </SortableHeader>
                )}
                <SortableHeader
                  field="members"
                  current={sortField}
                  dir={sortDir}
                  onSort={toggleSort}
                >
                  Members
                </SortableHeader>
              </TableRow>
            </TableHeader>
            <TableBody>
              {sortedMetrics.map((m) => {
                const childTeam = selectedTeam.children.find((c) => c.id === m.teamId);
                return (
                  <MetricsRow
                    key={m.teamId}
                    metrics={m}
                    hasChildren={(childTeam?.children.length ?? 0) > 0}
                    showDiscourse={hasDiscourse}
                  />
                );
              })}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      {/* Two separate charts instead of one mixed-axis chart */}
      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardContent className="pt-6">
            <h4 className="mb-3 text-sm font-medium">Throughput by Team</h4>
            <ResponsiveContainer width="100%" height={220}>
              <BarChart data={chartData} margin={{ top: 5, right: 10, left: 0, bottom: 5 }}>
                <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                <XAxis dataKey="name" tick={{ fontSize: 11 }} className="fill-muted-foreground" />
                <YAxis allowDecimals={false} className="fill-muted-foreground" />
                <Tooltip content={ChartTooltip} cursor={cursorStyle} />
                <Bar
                  dataKey="throughput"
                  name="Merged PRs"
                  fill="hsl(var(--primary))"
                  radius={[4, 4, 0, 0]}
                />
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
        <Card>
          <CardContent className="pt-6">
            <h4 className="mb-3 text-sm font-medium">Review P75 by Team</h4>
            <ResponsiveContainer width="100%" height={220}>
              <BarChart data={chartData} margin={{ top: 5, right: 10, left: 0, bottom: 5 }}>
                <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                <XAxis dataKey="name" tick={{ fontSize: 11 }} className="fill-muted-foreground" />
                <YAxis unit="h" className="fill-muted-foreground" />
                <Tooltip content={ChartTooltip} cursor={cursorStyle} />
                <Bar
                  dataKey="reviewP75Hours"
                  name="Review P75 (hrs)"
                  fill="hsl(var(--muted-foreground))"
                  radius={[4, 4, 0, 0]}
                />
              </BarChart>
            </ResponsiveContainer>
          </CardContent>
        </Card>
      </div>
    </>
  );
};
