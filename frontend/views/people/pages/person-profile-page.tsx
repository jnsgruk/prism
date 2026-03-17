import { useParams, useNavigate, Link } from "react-router-dom";
import { useState } from "react";
import {
  ArrowLeft,
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
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import {
  PeriodSelector,
  buildPeriod,
  defaultPeriodKey,
} from "@/views/teams/components/period-selector";
import { ContributionTable } from "@/views/teams/components/contribution-table";
import { useGetIndividualProfile } from "@/lib/hooks/use-metrics";
import { ProfileMetricCards } from "@/views/people/components/profile-metric-cards";
import { ActivityChart } from "@/views/people/components/activity-chart";
import { PeerContextPanel } from "@/views/people/components/peer-context-panel";

const PersonProfilePage = (): React.ReactElement => {
  const { personId } = useParams<{ personId: string }>();
  const navigate = useNavigate();
  const [periodKey, setPeriodKey] = useState(defaultPeriodKey);
  const period = buildPeriod(periodKey);
  const [prsOpen, setPrsOpen] = useState(false);
  const [reviewsOpen, setReviewsOpen] = useState(false);
  const [discourseOpen, setDiscourseOpen] = useState(false);
  const [identitiesOpen, setIdentitiesOpen] = useState(false);

  const { data: profile, isLoading, error } = useGetIndividualProfile(personId ?? "", period);

  if (!personId) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <p className="text-muted-foreground">No person selected.</p>
      </div>
    );
  }

  const description = [profile?.teamName, profile?.level].filter(Boolean).join(" \u00b7 ");

  const github = profile?.activityByPlatform.find((a) => a.platform === "github");
  const prCount = github?.metrics["pull_request_count"] ?? 0;
  const reviewCount = github?.metrics["pr_review_count"] ?? 0;
  const discourseCount =
    profile?.activityByPlatform
      .filter((a) => a.platform.startsWith("discourse"))
      .reduce((sum, a) => sum + a.contributionCount, 0) ?? 0;

  return (
    <>
      <PageHeader
        title={profile?.name ?? "Person"}
        description={description || undefined}
        actions={
          <button
            type="button"
            onClick={() => navigate(-1)}
            className="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
          >
            <ArrowLeft className="size-4" />
            Back
          </button>
        }
      />
      <div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">
        {/* Period selector + breadcrumb */}
        <div className="space-y-3">
          <PeriodSelector value={periodKey} onChange={setPeriodKey} />
          {profile?.teamName && (
            <Breadcrumb>
              <BreadcrumbList>
                <BreadcrumbItem>
                  <BreadcrumbLink render={<Link to="/teams" />}>Teams</BreadcrumbLink>
                </BreadcrumbItem>
                <BreadcrumbSeparator />
                <BreadcrumbItem>
                  <BreadcrumbLink render={<Link to="/teams" />}>{profile.teamName}</BreadcrumbLink>
                </BreadcrumbItem>
                <BreadcrumbSeparator />
                <BreadcrumbItem>
                  <BreadcrumbPage>{profile.name}</BreadcrumbPage>
                </BreadcrumbItem>
              </BreadcrumbList>
            </Breadcrumb>
          )}
        </div>

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
                    {prCount > 0 && (
                      <Badge variant="secondary" className="ml-1">
                        {prCount}
                      </Badge>
                    )}
                  </CollapsibleTrigger>
                </CardHeader>
                <CollapsibleContent>
                  <CardContent className="pt-0">
                    <ContributionTable
                      personId={personId}
                      defaultContributionType="pull_request"
                      defaultState="merged"
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
                    {reviewCount > 0 && (
                      <Badge variant="secondary" className="ml-1">
                        {reviewCount}
                      </Badge>
                    )}
                  </CollapsibleTrigger>
                </CardHeader>
                <CollapsibleContent>
                  <CardContent className="pt-0">
                    <ContributionTable personId={personId} defaultContributionType="pr_review" />
                  </CardContent>
                </CollapsibleContent>
              </Card>
            </Collapsible>

            {/* Discourse — collapsible, only if person has discourse activity */}
            {discourseCount > 0 && (
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
                      <Badge variant="secondary" className="ml-1">
                        {discourseCount}
                      </Badge>
                    </CollapsibleTrigger>
                  </CardHeader>
                  <CollapsibleContent>
                    <CardContent className="pt-0">
                      <ContributionTable personId={personId} defaultPlatform="discourse-%" />
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
