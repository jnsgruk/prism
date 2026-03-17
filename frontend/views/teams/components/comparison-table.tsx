import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Table, TableBody, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { useMemo, useState } from "react";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";
import type { Team } from "@ps/api/gen/prism/v1/org_pb";

import { MetricsRow } from "@/views/teams/components/metrics-row";
import { SortableHeader } from "@/views/teams/components/sortable-header";
import type { SortDir, SortField } from "@/views/teams/components/sortable-header";
import { teamTypeBadgeVariant, teamTypeLabel } from "@/views/teams/hooks/use-teams";

export const ComparisonTable = ({
  childMetrics,
  selectedTeam,
}: {
  childMetrics: TeamMetrics[];
  selectedTeam: Team;
}): React.ReactElement => {
  const [sortField, setSortField] = useState<SortField>("throughput");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

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
        case "wip":
          return dir * (a.wipAvg - b.wipAvg);
        case "leadTime":
          return dir * (a.leadTimeHours - b.leadTimeHours);
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
                <TableHead className="w-20">Type</TableHead>
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
                <SortableHeader field="wip" current={sortField} dir={sortDir} onSort={toggleSort}>
                  WIP
                </SortableHeader>
                <SortableHeader
                  field="leadTime"
                  current={sortField}
                  dir={sortDir}
                  onSort={toggleSort}
                >
                  Lead Time
                </SortableHeader>
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
                    teamType={childTeam ? teamTypeLabel(childTeam.teamType) : undefined}
                    teamTypeBadge={childTeam ? teamTypeBadgeVariant(childTeam.teamType) : undefined}
                    hasChildren={(childTeam?.children.length ?? 0) > 0}
                  />
                );
              })}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle>Throughput by Team</CardTitle>
        </CardHeader>
        <CardContent>
          <ResponsiveContainer width="100%" height={300}>
            <BarChart data={chartData} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
              <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
              <XAxis dataKey="name" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
              <YAxis className="fill-muted-foreground" />
              <Tooltip
                contentStyle={{
                  backgroundColor: "hsl(var(--popover))",
                  border: "1px solid hsl(var(--border))",
                  borderRadius: "var(--radius)",
                  color: "hsl(var(--popover-foreground))",
                }}
              />
              <Bar
                dataKey="throughput"
                name="Throughput"
                fill="hsl(var(--primary))"
                radius={[4, 4, 0, 0]}
              />
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
    </>
  );
};
