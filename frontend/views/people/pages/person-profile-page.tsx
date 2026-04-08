import { useParams, useNavigate } from "react-router-dom";
import { useMemo, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  Clock,
  GitPullRequest,
  KeyRound,
  Loader2,
  MessageSquare,
} from "lucide-react";

import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Alert, AlertDescription } from "@/components/ui/alert";
import {
  PeriodSelector,
  buildPeriod,
  defaultPeriodKey,
} from "@/views/teams/components/period-selector";
import { ContributionTable } from "@/views/teams/components/contribution-table";
import { ContributionType, Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { useGetIndividualProfile, usePersonContributionCount } from "@/lib/hooks/use-metrics";
import { PersonBreadcrumb } from "@/views/people/components/person-breadcrumb";
import { ProfileMetricCards } from "@/views/people/components/profile-metric-cards";
import { ActivityChart } from "@/views/people/components/activity-chart";
import { PeerContextPanel } from "@/views/people/components/peer-context-panel";
import { PersonInsightsSection } from "@/views/people/components/person-insights-section";
import { usePersonInsights } from "@/views/people/hooks/use-insights";

const PersonProfilePage = (): React.ReactElement => {
  const { personId } = useParams<{ personId: string }>();
  const navigate = useNavigate();
  const [periodKey, setPeriodKey] = useState(defaultPeriodKey);
  const period = buildPeriod(periodKey);
  const [prsOpen, setPrsOpen] = useState(false);
  const [reviewsOpen, setReviewsOpen] = useState(false);
  const [discourseOpen, setDiscourseOpen] = useState(false);
  const [identitiesOpen, setIdentitiesOpen] = useState(false);

  const safeId = personId ?? "";
  const periodFilters = useMemo(
    () => ({ since: period.start || undefined, until: period.end || undefined }),
    [period.start, period.end],
  );

  const { data: profile, isLoading, error } = useGetIndividualProfile(safeId, period);
  const {
    data: insights,
    isLoading: insightsLoading,
    error: insightsError,
  } = usePersonInsights(safeId, periodKey);

  const { data: prTotalCount } = usePersonContributionCount(safeId, {
    ...periodFilters,
    contributionType: ContributionType.PULL_REQUEST,
  });
  const { data: reviewTotalCount } = usePersonContributionCount(safeId, {
    ...periodFilters,
    contributionType: ContributionType.PR_REVIEW,
  });
  const { data: discourseTotalCount } = usePersonContributionCount(safeId, {
    ...periodFilters,
    platform: Platform.DISCOURSE,
  });

  if (!personId) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <p className="text-muted-foreground">No person selected.</p>
      </div>
    );
  }

  const hasDiscourseActivity =
    profile?.activityByPlatform.some((a) => a.platform === Platform.DISCOURSE) ?? false;

  return (
    <>
      <PageHeader
        title={
          <div className="flex items-center gap-2">
            <PersonBreadcrumb
              personName={profile?.name ?? "Person"}
              personId={personId}
              onSelect={(id) => {
                if (id === "__all__") {
                  navigate("/people");
                } else {
                  navigate(`/people/${id}`);
                }
              }}
            />
            {profile?.teamName && (
              <Badge variant="secondary" className="text-[10px]">
                {profile.teamName}
              </Badge>
            )}
            {profile?.level && (
              <Badge variant="secondary" className="text-[10px]">
                {profile.level}
              </Badge>
            )}
          </div>
        }
      />
      <div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">
        <PeriodSelector value={periodKey} onChange={setPeriodKey} />

        {/* Loading */}
        {isLoading && (
          <div className="flex justify-center py-12">
            <Loader2 className="size-6 animate-spin text-muted-foreground" />
          </div>
        )}

        {/* Error */}
        {error && (
          <Alert variant="destructive">
            <AlertDescription>
              {error instanceof Error ? error.message : "Failed to load profile"}
            </AlertDescription>
          </Alert>
        )}

        {/* Profile content */}
        {profile && !isLoading && (
          <>
            {/* Metric cards */}
            <ProfileMetricCards profile={profile} />

            {/* Insights */}
            <PersonInsightsSection
              insights={insights}
              isLoading={insightsLoading}
              error={insightsError}
            />

            {/* Activity chart */}
            <ActivityChart profile={profile} />

            {/* Peer context */}
            <PeerContextPanel profile={profile} />

            {/* Pull Requests — collapsible */}
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
                    {prTotalCount !== undefined && prTotalCount > 0 && (
                      <Badge variant="secondary" className="ml-1">
                        {prTotalCount}
                      </Badge>
                    )}
                  </CollapsibleTrigger>
                </CardHeader>
                <CollapsibleContent>
                  <CardContent className="pt-0">
                    <ContributionTable
                      personId={personId}
                      period={period}
                      defaultContributionType={ContributionType.PULL_REQUEST}
                    />
                  </CardContent>
                </CollapsibleContent>
              </Card>
            </Collapsible>

            {/* Reviews — collapsible */}
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
                    {reviewTotalCount !== undefined && reviewTotalCount > 0 && (
                      <Badge variant="secondary" className="ml-1">
                        {reviewTotalCount}
                      </Badge>
                    )}
                  </CollapsibleTrigger>
                </CardHeader>
                <CollapsibleContent>
                  <CardContent className="pt-0">
                    <ContributionTable
                      personId={personId}
                      period={period}
                      defaultContributionType={ContributionType.PR_REVIEW}
                    />
                  </CardContent>
                </CollapsibleContent>
              </Card>
            </Collapsible>

            {/* Discourse — collapsible, only if person has discourse activity */}
            {hasDiscourseActivity && (
              <Collapsible open={discourseOpen} onOpenChange={setDiscourseOpen}>
                <Card>
                  <CardHeader
                    className="cursor-pointer"
                    onClick={() => setDiscourseOpen(!discourseOpen)}
                  >
                    <CollapsibleTrigger
                      render={
                        <button
                          type="button"
                          className="flex w-full items-center gap-2 text-left"
                        />
                      }
                    >
                      {discourseOpen ? (
                        <ChevronDown className="size-4" />
                      ) : (
                        <ChevronRight className="size-4" />
                      )}
                      <MessageSquare className="size-4 text-muted-foreground" />
                      <CardTitle>Discourse</CardTitle>
                      {discourseTotalCount !== undefined && discourseTotalCount > 0 && (
                        <Badge variant="secondary" className="ml-1">
                          {discourseTotalCount}
                        </Badge>
                      )}
                    </CollapsibleTrigger>
                  </CardHeader>
                  <CollapsibleContent>
                    <CardContent className="pt-0">
                      <ContributionTable
                        personId={personId}
                        period={period}
                        defaultPlatform={Platform.DISCOURSE}
                      />
                    </CardContent>
                  </CollapsibleContent>
                </Card>
              </Collapsible>
            )}

            {/* Identities — collapsible */}
            {profile.identities.length > 0 && (
              <Collapsible open={identitiesOpen} onOpenChange={setIdentitiesOpen}>
                <Card>
                  <CardHeader
                    className="cursor-pointer"
                    onClick={() => setIdentitiesOpen(!identitiesOpen)}
                  >
                    <CollapsibleTrigger
                      render={
                        <button
                          type="button"
                          className="flex w-full items-center gap-2 text-left"
                        />
                      }
                    >
                      {identitiesOpen ? (
                        <ChevronDown className="size-4" />
                      ) : (
                        <ChevronRight className="size-4" />
                      )}
                      <KeyRound className="size-4 text-muted-foreground" />
                      <CardTitle>Identities</CardTitle>
                      <Badge variant="secondary" className="ml-1">
                        {profile.identities.length}
                      </Badge>
                    </CollapsibleTrigger>
                  </CardHeader>
                  <CollapsibleContent>
                    <CardContent className="pt-0">
                      <div className="space-y-1">
                        {profile.identities.map((id) => (
                          <div
                            key={`${id.platform}-${id.username}`}
                            className="flex items-center justify-between rounded-md border px-3 py-2 text-sm"
                          >
                            <span className="font-medium capitalize">{id.platform}</span>
                            <span className="text-muted-foreground">{id.username}</span>
                          </div>
                        ))}
                      </div>
                    </CardContent>
                  </CollapsibleContent>
                </Card>
              </Collapsible>
            )}
          </>
        )}
      </div>
    </>
  );
};

export default PersonProfilePage;
