import { create } from "@bufbuild/protobuf";
import { PageHeader } from "@/components/page-header";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { AlertCircle, ArrowUpDown, Users } from "lucide-react";
import { useMemo, useState } from "react";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";

import type { Period, TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";
import { PeriodSchema, PeriodType } from "@ps/api/gen/prism/v1/metrics_pb";

import { useCompareTeams } from "@/lib/hooks/use-metrics";
import { ImportDirectoryDialog } from "@/views/teams/components/import-directory-dialog";
import { PeriodSelector } from "@/views/teams/components/period-selector";
import { TeamDetailPanel } from "@/views/teams/components/team-detail-panel";
import { useListTeams } from "@/views/teams/hooks/use-teams";

type SortField = "name" | "throughput" | "review" | "members";
type SortDir = "asc" | "desc";

const defaultPeriod = (): Period => {
  const now = new Date();
  const start = new Date(now.getFullYear(), now.getMonth(), 1);
  const end = new Date(now.getFullYear(), now.getMonth() + 1, 0);
  return create(PeriodSchema, {
    type: PeriodType.MONTH,
    start: start.toISOString().slice(0, 10),
    end: end.toISOString().slice(0, 10),
  });
};

const TeamsPage = (): React.ReactElement => {
  const [selectedTeamId, setSelectedTeamId] = useState<string | null>(null);
  const [period, setPeriod] = useState<Period>(defaultPeriod);
  const [sortField, setSortField] = useState<SortField>("throughput");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  const { data: teams, isLoading: teamsLoading, error: teamsError } = useListTeams();

  const teamIds = useMemo(() => teams?.map((t) => t.id) ?? [], [teams]);
  const {
    data: metrics,
    isLoading: metricsLoading,
    error: metricsError,
  } = useCompareTeams(teamIds, period);

  const toggleSort = (field: SortField): void => {
    if (sortField === field) {
      setSortDir(sortDir === "asc" ? "desc" : "asc");
    } else {
      setSortField(field);
      setSortDir("desc");
    }
  };

  const sortedMetrics = useMemo(() => {
    if (!metrics) return [];
    const sorted = [...metrics];
    const dir = sortDir === "asc" ? 1 : -1;
    sorted.sort((a, b) => {
      switch (sortField) {
        case "name":
          return dir * a.teamName.localeCompare(b.teamName);
        case "throughput":
          return dir * (a.throughput - b.throughput);
        case "review":
          return dir * (a.avgReviewTurnaroundHours - b.avgReviewTurnaroundHours);
        case "members":
          return dir * (a.memberCount - b.memberCount);
        default:
          return 0;
      }
    });
    return sorted;
  }, [metrics, sortField, sortDir]);

  const chartData = useMemo(
    () =>
      sortedMetrics.map((m) => ({
        name: m.teamName,
        throughput: m.throughput,
        avgReviewHours: Math.round(m.avgReviewTurnaroundHours * 10) / 10,
      })),
    [sortedMetrics],
  );

  const isLoading = teamsLoading || metricsLoading;
  const error = teamsError ?? metricsError;

  return (
    <>
      <PageHeader
        title="Teams"
        description="Compare team performance across PR throughput and review turnaround"
        actions={
          <div className="flex items-center gap-3">
            <PeriodSelector value={period} onChange={setPeriod} />
            <ImportDirectoryDialog />
          </div>
        }
      />
      <div className="flex-1 space-y-6 p-6">
        {isLoading && <p className="text-sm text-muted-foreground">Loading metrics...</p>}

        {error && (
          <Alert variant="destructive">
            <AlertCircle className="size-4" />
            Failed to load team metrics.
          </Alert>
        )}

        {teams && teams.length === 0 && (
          <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
            <Users className="mb-3 size-10 text-muted-foreground" />
            <p className="mb-1 font-medium">No teams yet</p>
            <p className="text-sm text-muted-foreground">Import a directory file to get started.</p>
          </div>
        )}

        {sortedMetrics.length > 0 && (
          <>
            {/* Bar chart comparing throughput */}
            <Card>
              <CardHeader>
                <CardTitle>PR Throughput by Team</CardTitle>
              </CardHeader>
              <CardContent>
                <ResponsiveContainer width="100%" height={300}>
                  <BarChart data={chartData} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
                    <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                    <XAxis
                      dataKey="name"
                      tick={{ fontSize: 12 }}
                      className="fill-muted-foreground"
                    />
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
                      name="Merged PRs"
                      fill="hsl(var(--primary))"
                      radius={[4, 4, 0, 0]}
                    />
                    <Bar
                      dataKey="avgReviewHours"
                      name="Avg Review (hrs)"
                      fill="hsl(var(--muted-foreground))"
                      radius={[4, 4, 0, 0]}
                    />
                  </BarChart>
                </ResponsiveContainer>
              </CardContent>
            </Card>

            {/* Comparison table + detail panel */}
            <div className="grid gap-6 lg:grid-cols-5">
              <div className="lg:col-span-3">
                <Card>
                  <CardContent className="p-0">
                    <Table>
                      <TableHeader>
                        <TableRow>
                          <SortableHeader
                            field="name"
                            current={sortField}
                            dir={sortDir}
                            onSort={toggleSort}
                          >
                            Team
                          </SortableHeader>
                          <SortableHeader
                            field="throughput"
                            current={sortField}
                            dir={sortDir}
                            onSort={toggleSort}
                          >
                            Merged PRs
                          </SortableHeader>
                          <SortableHeader
                            field="review"
                            current={sortField}
                            dir={sortDir}
                            onSort={toggleSort}
                          >
                            Avg Review (hrs)
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
                        {sortedMetrics.map((m) => (
                          <MetricsRow
                            key={m.teamId}
                            metrics={m}
                            isSelected={selectedTeamId === m.teamId}
                            onSelect={() => setSelectedTeamId(m.teamId)}
                          />
                        ))}
                      </TableBody>
                    </Table>
                  </CardContent>
                </Card>
              </div>

              <div className="lg:col-span-2">
                {selectedTeamId ? (
                  <TeamDetailPanel
                    teamId={selectedTeamId}
                    onClose={() => setSelectedTeamId(null)}
                  />
                ) : (
                  <div className="flex h-full items-center justify-center rounded-lg border-2 border-dashed p-12">
                    <p className="text-sm text-muted-foreground">
                      Select a team to view its members.
                    </p>
                  </div>
                )}
              </div>
            </div>
          </>
        )}
      </div>
    </>
  );
};

const SortableHeader = ({
  field,
  current,
  dir,
  onSort,
  children,
}: {
  field: SortField;
  current: SortField;
  dir: SortDir;
  onSort: (field: SortField) => void;
  children: React.ReactNode;
}): React.ReactElement => (
  <TableHead>
    <button className="flex items-center gap-1 text-left font-medium" onClick={() => onSort(field)}>
      {children}
      <ArrowUpDown
        className={`size-3 ${current === field ? "text-foreground" : "text-muted-foreground/50"}`}
      />
      {current === field && <span className="text-xs">{dir === "asc" ? "\u2191" : "\u2193"}</span>}
    </button>
  </TableHead>
);

const MetricsRow = ({
  metrics,
  isSelected,
  onSelect,
}: {
  metrics: TeamMetrics;
  isSelected: boolean;
  onSelect: () => void;
}): React.ReactElement => (
  <TableRow className={`cursor-pointer ${isSelected ? "bg-muted/50" : ""}`} onClick={onSelect}>
    <TableCell className="font-medium">{metrics.teamName}</TableCell>
    <TableCell>
      <Badge variant={metrics.throughput > 0 ? "default" : "secondary"}>{metrics.throughput}</Badge>
    </TableCell>
    <TableCell>
      {metrics.avgReviewTurnaroundHours > 0
        ? `${metrics.avgReviewTurnaroundHours.toFixed(1)}h`
        : "\u2014"}
    </TableCell>
    <TableCell>{metrics.memberCount}</TableCell>
  </TableRow>
);

export default TeamsPage;
