import { PageHeader } from "@/components/page-header";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { AlertCircle, ArrowUpDown, ChevronDown, ChevronRight, Users } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useSearchParams } from "react-router";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";

import { useCompareTeams } from "@/lib/hooks/use-metrics";
import { TeamBreadcrumb } from "@/views/teams/components/team-breadcrumb";
import { TeamMetricCards } from "@/views/teams/components/team-metric-cards";
import {
  MetricDrilldownSheet,
  type DrilldownMetric,
  type DrilldownTarget,
} from "@/views/teams/components/metric-drilldown-sheet";
import {
  buildPeriod,
  defaultPeriodKey,
  PeriodSelector,
} from "@/views/teams/components/period-selector";
import { TeamSelector } from "@/views/teams/components/team-selector";
import {
  findTeam,
  teamTypeBadgeVariant,
  teamTypeLabel,
  useGetTeam,
  useGetTeamTree,
} from "@/views/teams/hooks/use-teams";

type SortField = "name" | "throughput" | "reviewP75" | "members";
type SortDir = "asc" | "desc";

const TeamsPage = (): React.ReactElement => {
  const [searchParams, setSearchParams] = useSearchParams();
  const selectedTeamId = searchParams.get("team");
  const periodKey = searchParams.get("period") ?? defaultPeriodKey;

  const setSelectedTeamId = (id: string): void => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev);
      next.set("team", id);
      return next;
    });
  };

  const setPeriodKey = (key: string): void => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev);
      next.set("period", key);
      return next;
    });
  };

  const [sortField, setSortField] = useState<SortField>("throughput");
  const [sortDir, setSortDir] = useState<SortDir>("desc");
  const [drilldown, setDrilldown] = useState<DrilldownTarget | null>(null);

  const period = useMemo(() => buildPeriod(periodKey), [periodKey]);

  const { data: tree, isLoading: treeLoading, error: treeError } = useGetTeamTree();

  const roots = useMemo(() => tree?.roots ?? [], [tree]);

  // Default to first root if no team selected
  const effectiveTeamId = selectedTeamId ?? roots[0]?.id ?? "";
  const selectedTeam = useMemo(
    () => (effectiveTeamId ? findTeam(roots, effectiveTeamId) : undefined),
    [roots, effectiveTeamId],
  );

  // Fetch children metrics
  const childIds = useMemo(() => selectedTeam?.children.map((c) => c.id) ?? [], [selectedTeam]);
  const {
    data: childMetrics,
    isLoading: metricsLoading,
    error: metricsError,
  } = useCompareTeams(childIds, period);

  // Also fetch the selected team's own metrics for the summary cards
  const teamIdArray = useMemo(() => (effectiveTeamId ? [effectiveTeamId] : []), [effectiveTeamId]);
  const { data: parentMetrics } = useCompareTeams(teamIdArray, period);
  const currentMetrics = parentMetrics?.[0];

  // Fetch members for the selected team
  const { data: teamDetail } = useGetTeam(effectiveTeamId);

  const toggleSort = (field: SortField): void => {
    if (sortField === field) {
      setSortDir(sortDir === "asc" ? "desc" : "asc");
    } else {
      setSortField(field);
      setSortDir("desc");
    }
  };

  const sortedMetrics = useMemo(() => {
    if (!childMetrics) return [];
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

  const isLoading = treeLoading || metricsLoading;
  const error = treeError ?? metricsError;

  const [membersOpen, setMembersOpen] = useState(false);
  const hasChildren = (selectedTeam?.children.length ?? 0) > 0;
  const members = teamDetail?.members ?? [];

  const openDrilldown = useCallback((metric: DrilldownMetric, teamId: string, teamName: string) => {
    setDrilldown({ metric, teamId, teamName });
  }, []);

  const closeDrilldown = useCallback(() => setDrilldown(null), []);

  return (
    <>
      <PageHeader
        title="Teams"
        description="Organisation hierarchy and team performance"
        actions={
          <TeamSelector roots={roots} selectedTeam={selectedTeam} onSelect={setSelectedTeamId} />
        }
      />
      <div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">
        {/* Navigation: period selector, breadcrumbs (when nested) */}
        <div className="space-y-3">
          <PeriodSelector value={periodKey} onChange={setPeriodKey} />
          {effectiveTeamId && roots.length > 0 && (
            <TeamBreadcrumb
              roots={roots}
              selectedTeamId={effectiveTeamId}
              onSelect={setSelectedTeamId}
            />
          )}
        </div>

        {isLoading && <p className="text-sm text-muted-foreground">Loading...</p>}

        {error && (
          <Alert variant="destructive">
            <AlertCircle className="size-4" />
            Failed to load team data.
          </Alert>
        )}

        {/* Metric summary cards */}
        {selectedTeam && (
          <TeamMetricCards
            metrics={currentMetrics}
            memberCount={
              selectedTeam.totalMemberCount > 0
                ? selectedTeam.totalMemberCount
                : selectedTeam.memberCount
            }
            onDrillDown={(metric) => openDrilldown(metric, effectiveTeamId, selectedTeam.name)}
          />
        )}

        {/* Child teams comparison table */}
        {sortedMetrics.length > 0 && (
          <>
            <Card>
              <CardContent className="overflow-x-auto p-0">
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
                      <TableHead className="w-20">Type</TableHead>
                      <SortableHeader
                        field="throughput"
                        current={sortField}
                        dir={sortDir}
                        onSort={toggleSort}
                      >
                        Merged PRs
                      </SortableHeader>
                      <SortableHeader
                        field="reviewP75"
                        current={sortField}
                        dir={sortDir}
                        onSort={toggleSort}
                      >
                        Review P75 (hrs)
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
                      const childTeam = selectedTeam?.children.find((c) => c.id === m.teamId);
                      return (
                        <MetricsRow
                          key={m.teamId}
                          metrics={m}
                          teamType={childTeam ? teamTypeLabel(childTeam.teamType) : undefined}
                          teamTypeBadge={
                            childTeam ? teamTypeBadgeVariant(childTeam.teamType) : undefined
                          }
                          hasChildren={(childTeam?.children.length ?? 0) > 0}
                          onSelect={() => setSelectedTeamId(m.teamId)}
                          onDrillDown={(metric) => openDrilldown(metric, m.teamId, m.teamName)}
                        />
                      );
                    })}
                  </TableBody>
                </Table>
              </CardContent>
            </Card>

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
        )}

        {/* No children message for leaf teams */}
        {selectedTeam && !hasChildren && !isLoading && sortedMetrics.length === 0 && (
          <Card>
            <CardContent className="p-6">
              <p className="text-sm text-muted-foreground">
                This is a leaf team with no sub-teams. Member details are shown below.
              </p>
            </CardContent>
          </Card>
        )}

        {/* Collapsible members section */}
        {selectedTeam && members.length > 0 && (
          <Collapsible open={!hasChildren || membersOpen} onOpenChange={setMembersOpen}>
            <Card>
              <CardHeader className="cursor-pointer" onClick={() => setMembersOpen(!membersOpen)}>
                <CollapsibleTrigger
                  render={
                    <button type="button" className="flex w-full items-center gap-2 text-left" />
                  }
                >
                  {hasChildren && membersOpen && <ChevronDown className="size-4" />}
                  {hasChildren && !membersOpen && <ChevronRight className="size-4" />}
                  <CardTitle>Members ({members.length})</CardTitle>
                  <span className="flex items-center gap-1 text-xs text-muted-foreground">
                    <Users className="size-3" />
                    {selectedTeam.name}
                  </span>
                </CollapsibleTrigger>
              </CardHeader>
              <CollapsibleContent>
                <CardContent className="pt-0">
                  <div className="space-y-2">
                    {members.map((person) => (
                      <div
                        key={person.id}
                        className="flex flex-wrap items-center justify-between gap-2 rounded border px-4 py-3"
                      >
                        <div className="min-w-0">
                          <p className="truncate text-sm font-medium">{person.name}</p>
                          {person.email && (
                            <p className="truncate text-xs text-muted-foreground">{person.email}</p>
                          )}
                        </div>
                        <div className="flex flex-wrap gap-1">
                          {person.identities.map((id) => (
                            <Badge key={`${id.platform}-${id.username}`} variant="secondary">
                              {id.platform}
                            </Badge>
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                </CardContent>
              </CollapsibleContent>
            </Card>
          </Collapsible>
        )}
      </div>

      {/* Metric drill-down sheet */}
      <MetricDrilldownSheet target={drilldown} period={period} onClose={closeDrilldown} />
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
  teamType,
  teamTypeBadge,
  hasChildren,
  onSelect,
  onDrillDown,
}: {
  metrics: TeamMetrics;
  teamType: string | undefined;
  teamTypeBadge: "default" | "secondary" | "outline" | "destructive" | undefined;
  hasChildren: boolean;
  onSelect: () => void;
  onDrillDown: (metric: DrilldownMetric) => void;
}): React.ReactElement => (
  <TableRow className="cursor-pointer hover:bg-muted/50" onClick={onSelect}>
    <TableCell className="font-medium">
      <span className="flex items-center gap-2">
        {metrics.teamName}
        {hasChildren && <ChevronRight className="size-3 text-muted-foreground" />}
      </span>
    </TableCell>
    <TableCell>
      {teamType && teamTypeBadge && (
        <Badge variant={teamTypeBadge} className="text-[10px]">
          {teamType}
        </Badge>
      )}
    </TableCell>
    <TableCell>
      <button
        onClick={(e) => {
          e.stopPropagation();
          if (metrics.throughput > 0) onDrillDown("throughput");
        }}
        className={metrics.throughput > 0 ? "cursor-pointer" : "cursor-default"}
      >
        <Badge variant={metrics.throughput > 0 ? "default" : "secondary"}>
          {metrics.throughput}
        </Badge>
      </button>
    </TableCell>
    <TableCell>
      {metrics.reviewTurnaroundP75Hours > 0 ? (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onDrillDown("review_turnaround");
          }}
          className="cursor-pointer hover:underline"
        >
          {metrics.reviewTurnaroundP75Hours.toFixed(1)}h
        </button>
      ) : (
        "\u2014"
      )}
    </TableCell>
    <TableCell>{metrics.memberCount}</TableCell>
  </TableRow>
);

export default TeamsPage;
