import { useParams, useNavigate, Link } from "react-router-dom";
import { useState } from "react";
import {
  ArrowLeft,
  Activity,
  BarChart3,
  ChevronDown,
  ChevronRight,
  Clock,
  GitPullRequest,
  KeyRound,
  Loader2,
  MessageSquare,
  TrendingUp,
  Users,
} from "lucide-react";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { TooltipContentProps } from "recharts/types/component/Tooltip";

import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Tooltip as UITooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
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
import type { GetIndividualProfileResponse } from "@/lib/hooks/use-metrics";
import { useGetIndividualProfile } from "@/lib/hooks/use-metrics";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const fmtFloat = (v: number): string => (v > 0 ? v.toFixed(1) : "\u2014");
const fmtPercent = (v: number): string => `${Math.round(v * 100)}%`;

// ---------------------------------------------------------------------------
// Chart tooltip
// ---------------------------------------------------------------------------

const ChartTooltip = ({
  active,
  payload,
  label,
}: TooltipContentProps): React.ReactElement | null => {
  if (!active || !payload?.length) return null;
  return (
    <div className="rounded-md border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-md">
      <p className="mb-1 font-medium">{label}</p>
      {payload.map((entry) => (
        <p key={entry.name} className="text-muted-foreground">
          {entry.name}: {entry.value}
        </p>
      ))}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Metric cards
// ---------------------------------------------------------------------------

const MetricCard = ({
  label,
  value,
  icon: Icon,
  description,
}: {
  label: string;
  value: string;
  icon: React.ComponentType<{ className?: string }>;
  description?: string;
}): React.ReactElement => (
  <Card>
    <CardContent className="flex items-center gap-3 p-4">
      <div className="rounded-md bg-muted p-2">
        <Icon className="size-4 text-muted-foreground" />
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-2xl font-semibold leading-none">{value}</p>
        <div className="mt-1 flex items-center gap-1">
          <p className="text-xs text-muted-foreground">{label}</p>
          {description && (
            <UITooltip>
              <TooltipTrigger render={<button type="button" className="inline-flex shrink-0" />}>
                <Activity className="size-3 text-muted-foreground/50" />
              </TooltipTrigger>
              <TooltipContent side="bottom" className="max-w-64">
                {description}
              </TooltipContent>
            </UITooltip>
          )}
        </div>
      </div>
    </CardContent>
  </Card>
);

const ProfileMetricCards = ({
  profile,
}: {
  profile: GetIndividualProfileResponse;
}): React.ReactElement => {
  const totalContributions = profile.activityByPlatform.reduce(
    (sum, a) => sum + a.contributionCount,
    0,
  );
  const platformCount = profile.activityByPlatform.length;
  const peerPercentile = profile.peerContext?.metrics["throughput"];

  return (
    <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
      <MetricCard
        label="Contributions"
        value={String(totalContributions)}
        icon={Activity}
        description="Total contributions across all platforms in this period"
      />
      <MetricCard label="Platforms active" value={String(platformCount)} icon={BarChart3} />
      {peerPercentile ? (
        <MetricCard
          label="Peer percentile"
          value={fmtPercent(peerPercentile.percentile)}
          icon={TrendingUp}
          description={`Throughput percentile among ${profile.peerContext?.peerCount ?? 0} ${profile.peerContext?.level ?? ""} peers`}
        />
      ) : (
        <MetricCard label="Peer percentile" value="\u2014" icon={TrendingUp} />
      )}
      <MetricCard label="Identities" value={String(profile.identities.length)} icon={Users} />
    </div>
  );
};

// ---------------------------------------------------------------------------
// Activity by platform chart
// ---------------------------------------------------------------------------

const ActivityChart = ({
  profile,
}: {
  profile: GetIndividualProfileResponse;
}): React.ReactElement | null => {
  if (profile.activityByPlatform.length === 0) return null;

  const data = profile.activityByPlatform.map((a) => ({
    platform: a.platform,
    count: a.contributionCount,
  }));

  return (
    <Card>
      <CardHeader>
        <CardTitle>Activity by platform</CardTitle>
        <CardDescription>Contribution counts per platform in this period</CardDescription>
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={250}>
          <BarChart data={data} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
            <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
            <XAxis dataKey="platform" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
            <YAxis className="fill-muted-foreground" allowDecimals={false} />
            <Tooltip content={ChartTooltip} cursor={{ fill: "hsl(var(--muted))", opacity: 0.5 }} />
            <Bar
              dataKey="count"
              name="Contributions"
              fill="hsl(var(--primary))"
              radius={[4, 4, 0, 0]}
            />
          </BarChart>
        </ResponsiveContainer>
        {/* Per-platform key metrics */}
        <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {profile.activityByPlatform.map((a) => (
            <div key={a.platform} className="rounded-md border px-3 py-2">
              <p className="text-sm font-medium">{a.platform}</p>
              <p className="text-xs text-muted-foreground">
                {a.contributionCount} contribution{a.contributionCount !== 1 ? "s" : ""}
                {a.metrics["avg_review_hours"] != null &&
                  ` \u00b7 avg review ${fmtFloat(a.metrics["avg_review_hours"])}h`}
                {a.metrics["avg_cycle_time_hours"] != null &&
                  ` \u00b7 avg cycle ${fmtFloat(a.metrics["avg_cycle_time_hours"])}h`}
              </p>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
};

// ---------------------------------------------------------------------------
// Peer context panel
// ---------------------------------------------------------------------------

const PeerContextPanel = ({
  profile,
}: {
  profile: GetIndividualProfileResponse;
}): React.ReactElement | null => {
  const peer = profile.peerContext;
  if (!peer || peer.peerCount === 0) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Peer context</CardTitle>
        <CardDescription>
          Compared to {peer.peerCount} other {peer.level} peers in this period
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="space-y-2">
          {Object.entries(peer.metrics).map(([name, p]) => (
            <div
              key={name}
              className="flex items-center justify-between rounded-md border px-3 py-2"
            >
              <span className="text-sm capitalize">{name.replace(/_/g, " ")}</span>
              <div className="flex items-center gap-2">
                <span className="tabular-nums text-sm font-medium">{fmtFloat(p.value)}</span>
                <Badge variant="secondary" className="text-[10px]">
                  {fmtPercent(p.percentile)} percentile
                </Badge>
              </div>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
};

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

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
            {(() => {
              const github = profile.activityByPlatform.find((a) => a.platform === "github");
              const prCount = github?.metrics["pull_request_count"] ?? 0;
              const reviewCount = github?.metrics["pr_review_count"] ?? 0;
              const discourseCount = profile.activityByPlatform
                .filter((a) => a.platform.startsWith("discourse"))
                .reduce((sum, a) => sum + a.contributionCount, 0);

              return (
                <>
                  <Collapsible open={prsOpen} onOpenChange={setPrsOpen}>
                    <Card>
                      <CardHeader className="cursor-pointer" onClick={() => setPrsOpen(!prsOpen)}>
                        <CollapsibleTrigger
                          render={
                            <button
                              type="button"
                              className="flex w-full items-center gap-2 text-left"
                            />
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
                      <CardHeader
                        className="cursor-pointer"
                        onClick={() => setReviewsOpen(!reviewsOpen)}
                      >
                        <CollapsibleTrigger
                          render={
                            <button
                              type="button"
                              className="flex w-full items-center gap-2 text-left"
                            />
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
                          <ContributionTable
                            personId={personId}
                            defaultContributionType="pr_review"
                          />
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
                </>
              );
            })()}

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
