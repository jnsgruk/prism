import { PageHeader } from "@/components/page-header";
import { Alert } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  AlertCircle,
  ChevronDown,
  ChevronRight,
  Clock,
  GitPullRequest,
  Loader2,
  Users,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router";

import { useCompareTeams, useGetFlowMetrics } from "@/lib/hooks/use-metrics";
import { ComparisonTable } from "@/views/teams/components/comparison-table";
import { ContributionTable } from "@/views/teams/components/contribution-table";
import {
  buildPeriod,
  defaultPeriodKey,
  PeriodSelector,
} from "@/views/teams/components/period-selector";
import { ReviewDistribution } from "@/views/teams/components/review-distribution";
import { TeamBreadcrumb } from "@/views/teams/components/team-breadcrumb";
import { TeamMetricCards } from "@/views/teams/components/team-metric-cards";
import { ThroughputTrendChart, WipTrendChart } from "@/views/teams/components/trend-charts";
import { findTeam, useGetTeam, useGetTeamTree } from "@/views/teams/hooks/use-teams";

const TeamsPage = (): React.ReactElement => {
  const { teamId } = useParams<{ teamId: string }>();
  const navigate = useNavigate();
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

  const { data: tree, isLoading: treeLoading, error: treeError } = useGetTeamTree();
  const roots = useMemo(() => tree?.roots ?? [], [tree]);

  // Redirect /teams (no ID) to the first root team
  const firstRoot = roots[0];
  useEffect(() => {
    if (!teamId && firstRoot) {
      navigate(`/teams/${firstRoot.id}`, { replace: true });
    }
  }, [teamId, firstRoot, navigate]);

  const effectiveTeamId = teamId ?? roots[0]?.id ?? "";
  const selectedTeam = useMemo(
    () => (effectiveTeamId ? findTeam(roots, effectiveTeamId) : undefined),
    [roots, effectiveTeamId],
  );

  // Fetch children metrics for comparison table
  const childIds = useMemo(() => selectedTeam?.children.map((c) => c.id) ?? [], [selectedTeam]);
  const {
    data: childMetrics,
    isLoading: metricsLoading,
    error: metricsError,
  } = useCompareTeams(childIds, period);

  // Fetch the selected team's own metrics
  const teamIdArray = useMemo(() => (effectiveTeamId ? [effectiveTeamId] : []), [effectiveTeamId]);
  const { data: parentMetrics } = useCompareTeams(teamIdArray, period);
  const currentMetrics = parentMetrics?.[0];

  // Flow metrics for trend charts
  const { data: flowMetrics } = useGetFlowMetrics(effectiveTeamId, period);

  // Fetch members
  const { data: teamDetail } = useGetTeam(effectiveTeamId);

  const isLoading = treeLoading || metricsLoading;
  const error = treeError ?? metricsError;

  const [membersOpen, setMembersOpen] = useState(false);
  const [prsOpen, setPrsOpen] = useState(false);
  const [reviewsOpen, setReviewsOpen] = useState(false);
  const hasChildren = (selectedTeam?.children.length ?? 0) > 0;
  const members = teamDetail?.members ?? [];
  const teamName = selectedTeam?.name ?? currentMetrics?.teamName ?? "Teams";

  return (
    <>
      <PageHeader title={teamName} description="Team performance and contributions" />
      <div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">
        {/* Navigation: period selector, breadcrumbs */}
        <div className="space-y-3">
          <PeriodSelector value={periodKey} onChange={setPeriodKey} />
          {effectiveTeamId && roots.length > 0 && (
            <TeamBreadcrumb roots={roots} selectedTeamId={effectiveTeamId} />
          )}
        </div>

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

        {/* Metric summary cards */}
        {selectedTeam && (
          <TeamMetricCards
            metrics={currentMetrics}
            memberCount={
              selectedTeam.totalMemberCount > 0
                ? selectedTeam.totalMemberCount
                : selectedTeam.memberCount
            }
          />
        )}

        {/* Source platform badges */}
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

        {/* Child teams comparison table — right after cards */}
        {selectedTeam && (childMetrics?.length ?? 0) > 0 && (
          <ComparisonTable childMetrics={childMetrics ?? []} selectedTeam={selectedTeam} />
        )}

        {/* Trend charts — throughput + WIP */}
        <ThroughputTrendChart flowMetrics={flowMetrics} />
        <WipTrendChart flowMetrics={flowMetrics} />

        {/* Pull Requests — collapsible */}
        {selectedTeam && (
          <Collapsible open={prsOpen} onOpenChange={setPrsOpen}>
            <Card>
              <CardHeader className="cursor-pointer" onClick={() => setPrsOpen(!prsOpen)}>
                <CollapsibleTrigger
                  render={
                    <button type="button" className="flex w-full items-center gap-2 text-left" />
                  }
                >
                  {prsOpen ? (
                    <ChevronDown className="size-4" />
                  ) : (
                    <ChevronRight className="size-4" />
                  )}
                  <GitPullRequest className="size-4 text-muted-foreground" />
                  <CardTitle>Pull Requests</CardTitle>
                  {currentMetrics && (
                    <Badge variant="secondary" className="ml-1">
                      {currentMetrics.throughput}
                    </Badge>
                  )}
                </CollapsibleTrigger>
              </CardHeader>
              <CollapsibleContent>
                <CardContent className="pt-0">
                  <ContributionTable
                    teamId={effectiveTeamId}
                    period={period}
                    defaultContributionType="pull_request"
                    defaultState="merged"
                  />
                </CardContent>
              </CollapsibleContent>
            </Card>
          </Collapsible>
        )}

        {/* Reviews — collapsible */}
        {selectedTeam && (
          <Collapsible open={reviewsOpen} onOpenChange={setReviewsOpen}>
            <Card>
              <CardHeader className="cursor-pointer" onClick={() => setReviewsOpen(!reviewsOpen)}>
                <CollapsibleTrigger
                  render={
                    <button type="button" className="flex w-full items-center gap-2 text-left" />
                  }
                >
                  {reviewsOpen ? (
                    <ChevronDown className="size-4" />
                  ) : (
                    <ChevronRight className="size-4" />
                  )}
                  <Clock className="size-4 text-muted-foreground" />
                  <CardTitle>Reviews</CardTitle>
                  {currentMetrics && currentMetrics.reviewTurnaroundP75Hours > 0 && (
                    <Badge variant="secondary" className="ml-1">
                      P75 {currentMetrics.reviewTurnaroundP75Hours.toFixed(1)}h
                    </Badge>
                  )}
                </CollapsibleTrigger>
              </CardHeader>
              <CollapsibleContent>
                <CardContent className="space-y-4 pt-0">
                  <ReviewDistribution teamId={effectiveTeamId} period={period} />
                  <ContributionTable
                    teamId={effectiveTeamId}
                    period={period}
                    defaultContributionType="pr_review"
                  />
                </CardContent>
              </CollapsibleContent>
            </Card>
          </Collapsible>
        )}

        {/* Members — collapsible */}
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

export default TeamsPage;
