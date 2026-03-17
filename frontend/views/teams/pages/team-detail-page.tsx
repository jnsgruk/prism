import { PageHeader } from "@/components/page-header";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  AlertCircle,
  ArrowLeft,
  ChevronDown,
  ChevronRight,
  GitPullRequest,
  Loader2,
  Users,
} from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useParams, useSearchParams } from "react-router";
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

import { useCompareTeams, useGetFlowMetrics } from "@/lib/hooks/use-metrics";
import { ContributionTable } from "@/views/teams/components/contribution-table";
import {
  buildPeriod,
  defaultPeriodKey,
  PeriodSelector,
} from "@/views/teams/components/period-selector";
import { TeamMetricCards } from "@/views/teams/components/team-metric-cards";
import { useGetTeam } from "@/views/teams/hooks/use-teams";

const TeamDetailPage = (): React.ReactElement => {
  const { teamId } = useParams<{ teamId: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const periodKey = searchParams.get("period") ?? defaultPeriodKey;

  const setPeriodKey = (key: string): void => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev);
      next.set("period", key);
      return next;
    });
  };

  const period = useMemo(() => buildPeriod(periodKey), [periodKey]);

  const { data: teamDetail, isLoading: teamLoading, error: teamError } = useGetTeam(teamId ?? "");
  const teamIdArray = useMemo(() => (teamId ? [teamId] : []), [teamId]);
  const { data: metricsArray } = useCompareTeams(teamIdArray, period);
  const currentMetrics = metricsArray?.[0];

  const { data: flowMetrics } = useGetFlowMetrics(teamId ?? "", period);

  const members = teamDetail?.members ?? [];
  const teamName = teamDetail?.team?.name ?? currentMetrics?.teamName ?? "Team";

  const [prsOpen, setPrsOpen] = useState(true);
  const [membersOpen, setMembersOpen] = useState(false);

  const throughputTrend = useMemo(
    () =>
      (flowMetrics?.throughputTrend ?? []).map((t) => ({
        date: t.date,
        count: t.count,
      })),
    [flowMetrics],
  );

  const wipTrend = useMemo(
    () =>
      (flowMetrics?.wipTrend ?? []).map((w) => ({
        date: w.date,
        wip: Math.round(w.wip * 10) / 10,
      })),
    [flowMetrics],
  );

  const isLoading = teamLoading;
  const error = teamError;

  if (!teamId) {
    return (
      <div className="flex items-center justify-center p-12">
        <p className="text-muted-foreground">No team ID provided.</p>
      </div>
    );
  }

  return (
    <>
      <PageHeader
        title={teamName}
        description="Team metrics and contributions"
        actions={
          <Link
            to={`/teams${teamDetail?.team?.parentTeamId ? `?team=${teamDetail.team.parentTeamId}` : ""}`}
            className="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
          >
            <ArrowLeft className="size-4" />
            Back to teams
          </Link>
        }
      />
      <div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">
        <PeriodSelector value={periodKey} onChange={setPeriodKey} />

        {isLoading && (
          <div className="flex items-center justify-center p-12">
            <Loader2 className="size-6 animate-spin text-muted-foreground" />
          </div>
        )}

        {error && (
          <Alert variant="destructive">
            <AlertCircle className="size-4" />
            Failed to load team data.
          </Alert>
        )}

        {/* Metric cards */}
        <TeamMetricCards
          metrics={currentMetrics}
          memberCount={currentMetrics?.memberCount ?? members.length}
        />

        {/* Source platforms */}
        {currentMetrics && currentMetrics.sourcePlatforms.length > 0 && (
          <div className="flex items-center gap-2">
            <span className="text-sm text-muted-foreground">Sources:</span>
            {currentMetrics.sourcePlatforms.map((p) => (
              <Badge key={p} variant="outline">
                {p}
              </Badge>
            ))}
          </div>
        )}

        {/* Throughput trend chart */}
        {throughputTrend.length > 1 && (
          <Card>
            <CardHeader>
              <CardTitle>Throughput Trend</CardTitle>
            </CardHeader>
            <CardContent>
              <ResponsiveContainer width="100%" height={250}>
                <BarChart data={throughputTrend} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                  <XAxis dataKey="date" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
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
                    dataKey="count"
                    name="Completed items"
                    fill="hsl(var(--primary))"
                    radius={[4, 4, 0, 0]}
                  />
                </BarChart>
              </ResponsiveContainer>
            </CardContent>
          </Card>
        )}

        {/* WIP trend chart */}
        {wipTrend.length > 1 && (
          <Card>
            <CardHeader>
              <CardTitle>WIP Trend</CardTitle>
            </CardHeader>
            <CardContent>
              <ResponsiveContainer width="100%" height={250}>
                <LineChart data={wipTrend} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
                  <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                  <XAxis dataKey="date" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
                  <YAxis className="fill-muted-foreground" />
                  <Tooltip
                    contentStyle={{
                      backgroundColor: "hsl(var(--popover))",
                      border: "1px solid hsl(var(--border))",
                      borderRadius: "var(--radius)",
                      color: "hsl(var(--popover-foreground))",
                    }}
                  />
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
        )}

        {/* Contributions */}
        <Collapsible open={prsOpen} onOpenChange={setPrsOpen}>
          <Card>
            <CardHeader className="cursor-pointer" onClick={() => setPrsOpen(!prsOpen)}>
              <CollapsibleTrigger
                render={
                  <button type="button" className="flex w-full items-center gap-2 text-left" />
                }
              >
                {prsOpen ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
                <GitPullRequest className="size-4 text-muted-foreground" />
                <CardTitle>Contributions</CardTitle>
                {currentMetrics && (
                  <Badge variant="secondary" className="ml-1">
                    {currentMetrics.throughput}
                  </Badge>
                )}
              </CollapsibleTrigger>
            </CardHeader>
            <CollapsibleContent>
              <CardContent className="pt-0">
                <ContributionTable teamId={teamId} period={period} />
              </CardContent>
            </CollapsibleContent>
          </Card>
        </Collapsible>

        {/* Members */}
        {members.length > 0 && (
          <Collapsible open={membersOpen} onOpenChange={setMembersOpen}>
            <Card>
              <CardHeader className="cursor-pointer" onClick={() => setMembersOpen(!membersOpen)}>
                <CollapsibleTrigger
                  render={
                    <button type="button" className="flex w-full items-center gap-2 text-left" />
                  }
                >
                  {membersOpen ? (
                    <ChevronDown className="size-4" />
                  ) : (
                    <ChevronRight className="size-4" />
                  )}
                  <Users className="size-4 text-muted-foreground" />
                  <CardTitle>Members ({members.length})</CardTitle>
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
    </>
  );
};

export default TeamDetailPage;
