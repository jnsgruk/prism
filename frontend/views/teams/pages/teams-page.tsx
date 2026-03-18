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
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useParams, useSearchParams } from "react-router";

import { useCompareTeams, useGetFlowMetrics } from "@/lib/hooks/use-metrics";
import { CommunityPanel } from "@/views/teams/components/community-panel";
import { ComparisonTable } from "@/views/teams/components/comparison-table";
import { ContributionTable } from "@/views/teams/components/contribution-table";
import { DeliveryPanel } from "@/views/teams/components/delivery-panel";
import { DiscourseActivitySection } from "@/views/teams/components/discourse-activity-section";
import { FlowPanel } from "@/views/teams/components/flow-panel";
import {
  buildPeriod,
  defaultPeriodKey,
  PeriodSelector,
} from "@/views/teams/components/period-selector";
import { ReviewDistribution } from "@/views/teams/components/review-distribution";
import { TeamBreadcrumb } from "@/views/teams/components/team-breadcrumb";
import { TeamSelector } from "@/views/teams/components/team-selector";
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

  // Flow metrics for trend chart in delivery panel
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
  const memberCount =
    selectedTeam && selectedTeam.totalMemberCount > 0
      ? selectedTeam.totalMemberCount
      : (selectedTeam?.memberCount ?? 0);

  // Refs for scroll-to-section
  const prsRef = useRef<HTMLDivElement>(null);
  const reviewsRef = useRef<HTMLDivElement>(null);
  const discourseRef = useRef<HTMLDivElement>(null);
  const membersRef = useRef<HTMLDivElement>(null);

  const scrollToAndOpen = useCallback(
    (ref: React.RefObject<HTMLDivElement | null>, setOpen: (v: boolean) => void) => {
      setOpen(true);
      // Allow collapsible to open before scrolling
      requestAnimationFrame(() => {
        ref.current?.scrollIntoView({ behavior: "smooth", block: "start" });
      });
    },
    [],
  );

  return (
    <>
      <PageHeader
        title="Teams"
        description={
          effectiveTeamId && roots.length > 0 ? (
            <TeamBreadcrumb
              roots={roots}
              selectedTeamId={effectiveTeamId}
              selector={
                <TeamSelector
                  roots={roots}
                  selectedTeam={selectedTeam}
                  onSelect={(id) => navigate(`/teams/${id}`)}
                />
              }
            />
          ) : undefined
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

        {/* Themed metric panels */}
        {selectedTeam && (
          <DeliveryPanel
            metrics={currentMetrics}
            memberCount={memberCount}
            flowMetrics={flowMetrics}
            onScrollToPrs={() => scrollToAndOpen(prsRef, setPrsOpen)}
            onScrollToReviews={() => scrollToAndOpen(reviewsRef, setReviewsOpen)}
            onScrollToMembers={() => scrollToAndOpen(membersRef, setMembersOpen)}
          />
        )}

        {selectedTeam && <FlowPanel metrics={currentMetrics} />}

        {selectedTeam && (
          <CommunityPanel
            metrics={currentMetrics}
            onScrollToDiscourse={() => {
              discourseRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
            }}
          />
        )}

        {/* Child teams comparison table */}
        {selectedTeam && (childMetrics?.length ?? 0) > 0 && (
          <ComparisonTable
            childMetrics={childMetrics ?? []}
            selectedTeam={selectedTeam}
            sourcePlatforms={currentMetrics?.sourcePlatforms}
          />
        )}

        {/* Discourse Activity — collapsible, lazy-loaded */}
        {selectedTeam && (
          <div ref={discourseRef}>
            <DiscourseActivitySection
              teamId={effectiveTeamId}
              period={period}
              metrics={currentMetrics}
            />
          </div>
        )}

        {/* Pull Requests — collapsible */}
        {selectedTeam && (
          <div ref={prsRef}>
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
          </div>
        )}

        {/* Reviews — collapsible */}
        {selectedTeam && (
          <div ref={reviewsRef}>
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
          </div>
        )}

        {/* Members — collapsible */}
        {selectedTeam && members.length > 0 && (
          <div ref={membersRef}>
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
                        <button
                          key={person.id}
                          type="button"
                          onClick={() => navigate(`/people/${person.id}`)}
                          className="flex w-full cursor-pointer flex-wrap items-center justify-between gap-2 rounded border px-4 py-3 text-left hover:bg-muted/50"
                        >
                          <div className="min-w-0">
                            <p className="truncate text-sm font-medium">{person.name}</p>
                            {person.email && (
                              <p className="truncate text-xs text-muted-foreground">
                                {person.email}
                              </p>
                            )}
                          </div>
                          {person.identities.length > 0 && (
                            <Badge variant="secondary">
                              {person.identities.length}{" "}
                              {person.identities.length === 1 ? "identity" : "identities"}
                            </Badge>
                          )}
                        </button>
                      ))}
                    </div>
                  </CardContent>
                </CollapsibleContent>
              </Card>
            </Collapsible>
          </div>
        )}
      </div>
    </>
  );
};

export default TeamsPage;
