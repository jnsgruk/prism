import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { ArrowRight, Loader2, Plug, Sparkles } from "lucide-react";
import { useMemo } from "react";
import { Link, useSearchParams } from "react-router-dom";

import { useListSources } from "@ps/hooks";
import { useGetTeamTree } from "@/views/teams/hooks/use-teams";
import { PeriodSelector, defaultPeriodKey } from "@/views/teams/components/period-selector";
import { TeamBreadcrumb } from "@/views/teams/components/team-breadcrumb";
import { useOrgInsights } from "@/views/dashboard/hooks/use-insights";
import { DeliverySummaryCards } from "@/views/dashboard/components/delivery-summary-cards";
import { OrgInsightsSummary } from "@/views/dashboard/components/org-insights-summary";
import { TeamHealthGrid } from "@/views/dashboard/components/team-health-grid";
import { OrgHighlights } from "@/views/dashboard/components/org-highlights";

const DashboardPage = (): React.ReactElement => {
  const { data: sources, isLoading: sourcesLoading } = useListSources();
  const { data: treeData, isLoading: treeLoading } = useGetTeamTree();
  const [searchParams, setSearchParams] = useSearchParams();

  const periodKey = searchParams.get("period") ?? defaultPeriodKey;
  const teamIdParam = searchParams.get("team") ?? "";

  const roots = useMemo(() => treeData?.roots ?? [], [treeData]);

  // Default to first root team
  const selectedTeamId = useMemo(() => {
    if (teamIdParam && roots.length > 0) return teamIdParam;
    if (roots.length > 0) return roots[0]!.id;
    return "";
  }, [teamIdParam, roots]);

  const {
    data: insights,
    isLoading: insightsLoading,
    error: insightsError,
  } = useOrgInsights(periodKey, selectedTeamId || undefined);

  const hasSources = sources && sources.length > 0;

  const setPeriodKey = (key: string): void => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev);
      next.set("period", key);
      return next;
    });
  };

  const setTeamId = (id: string): void => {
    setSearchParams((prev) => {
      const next = new URLSearchParams(prev);
      next.set("team", id);
      return next;
    });
  };

  // No sources — onboarding
  if (!sourcesLoading && !hasSources) {
    return (
      <>
        <PageHeader title="Dashboard" />
        <div className="flex flex-1 items-center justify-center p-6">
          <Card className="mx-auto max-w-lg">
            <CardHeader className="text-center">
              <div className="mx-auto mb-2 flex size-12 items-center justify-center rounded-full bg-muted">
                <Plug className="size-6 text-muted-foreground" />
              </div>
              <CardTitle>Get started with Prism</CardTitle>
              <CardDescription>
                Connect your first data source to start gathering engineering insights across your
                team.
              </CardDescription>
            </CardHeader>
            <CardContent className="flex justify-center">
              <Button render={<Link to="/admin" />}>
                Configure Sources
                <ArrowRight className="size-4" />
              </Button>
            </CardContent>
          </Card>
        </div>
      </>
    );
  }

  // Loading state
  if (sourcesLoading || treeLoading) {
    return (
      <>
        <PageHeader title="Dashboard" />
        <div className="flex flex-1 items-center justify-center p-6">
          <Loader2 className="size-6 animate-spin text-muted-foreground" />
        </div>
      </>
    );
  }

  // No teams yet
  if (roots.length === 0) {
    return (
      <>
        <PageHeader title="Dashboard" description="Organisation overview" />
        <div className="flex-1 p-6">
          <Card className="mx-auto max-w-lg">
            <CardHeader className="text-center">
              <CardTitle>No teams configured</CardTitle>
              <CardDescription>
                {sources?.length ?? 0} source{(sources?.length ?? 0) !== 1 ? "s" : ""} connected.
                Run a team sync to populate your org structure, then metrics and insights will
                appear here.
              </CardDescription>
            </CardHeader>
          </Card>
        </div>
      </>
    );
  }

  return (
    <>
      <PageHeader
        title={
          <TeamBreadcrumb roots={roots} selectedTeamId={selectedTeamId} onSelect={setTeamId} />
        }
      />
      <div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">
        <PeriodSelector value={periodKey} onChange={setPeriodKey} />

        {/* Delivery Summary Cards */}
        {insightsLoading && (
          <div className="grid grid-cols-2 gap-4 lg:grid-cols-3 xl:grid-cols-6">
            {Array.from({ length: 6 }).map((_, i) => (
              <Skeleton key={i} className="h-24 w-full" />
            ))}
          </div>
        )}

        {insights?.delivery && <DeliverySummaryCards delivery={insights.delivery} />}

        {/* Insights Summary */}
        {insightsLoading && !insights && (
          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <Sparkles className="size-4 text-muted-foreground" />
                <CardTitle>Insights</CardTitle>
              </div>
            </CardHeader>
            <CardContent>
              <div className="flex items-center justify-center p-8">
                <Loader2 className="size-5 animate-spin text-muted-foreground" />
              </div>
            </CardContent>
          </Card>
        )}

        {insightsError && !insights && (
          <Card>
            <CardHeader>
              <div className="flex items-center gap-2">
                <Sparkles className="size-4 text-muted-foreground" />
                <CardTitle>Insights</CardTitle>
              </div>
            </CardHeader>
            <CardContent>
              <p className="text-sm text-muted-foreground">
                Failed to load insights: {insightsError.message}
              </p>
            </CardContent>
          </Card>
        )}

        {insights && <OrgInsightsSummary insights={insights} />}

        {/* Team Health Grid */}
        {insights && insights.teamComparison.length > 0 && (
          <TeamHealthGrid teams={insights.teamComparison} />
        )}

        {/* Notable Contributions */}
        {insights && <OrgHighlights highlights={insights.orgHighlights} />}
      </div>
    </>
  );
};

export default DashboardPage;
