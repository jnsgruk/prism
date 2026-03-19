import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { CategoryTags } from "@/components/category-tags";
import { CoverageIndicator } from "@/components/coverage-indicator";
import { DepthHistogram } from "@/components/depth-histogram";
import { NotableContributionCard } from "@/components/notable-contribution-card";
import { SentimentBar } from "@/components/sentiment-bar";
import { SignificanceBreakdown } from "@/components/significance-breakdown";
import { Info, Loader2, Sparkles } from "lucide-react";

import type { PersonInsights } from "@ps/api/gen/prism/v1/insights_pb";

const depthColor = (depth: number): string => {
  if (depth >= 2.8) return "text-emerald-600";
  if (depth >= 2.2) return "text-amber-600";
  return "text-red-600";
};

const MetricValue = ({
  value,
  label,
  description,
  colorClass,
}: {
  value: string;
  label: string;
  description: string;
  colorClass?: string;
}): React.ReactElement => (
  <div className="min-w-0 flex-1">
    <span className={`text-2xl font-semibold tabular-nums ${colorClass ?? ""}`}>{value}</span>
    <div className="mt-0.5 flex items-center gap-1">
      <span className="text-xs text-muted-foreground">{label}</span>
      <Tooltip>
        <TooltipTrigger render={<button type="button" className="inline-flex shrink-0" />}>
          <Info className="size-3 text-muted-foreground/50" />
        </TooltipTrigger>
        <TooltipContent side="bottom" className="max-w-64">
          {description}
        </TooltipContent>
      </Tooltip>
    </div>
  </div>
);

export const PersonInsightsSection = ({
  insights,
  isLoading,
  error,
}: {
  insights: PersonInsights | undefined;
  isLoading: boolean;
  error: Error | null;
}): React.ReactElement | null => {
  if (isLoading) {
    return (
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
    );
  }

  if (!insights) {
    if (error) {
      return (
        <Card>
          <CardHeader>
            <div className="flex items-center gap-2">
              <Sparkles className="size-4 text-muted-foreground" />
              <CardTitle>Insights</CardTitle>
            </div>
          </CardHeader>
          <CardContent>
            <p className="text-sm text-muted-foreground">
              Failed to load insights: {error.message}
            </p>
          </CardContent>
        </Card>
      );
    }
    return null;
  }

  const coverage = insights.coverage;
  const reviewer = insights.reviewerProfile;
  const received = insights.reviewsReceived;
  const sig = insights.prImpact;
  const topics = insights.discourseTopics;
  const highlights = insights.highlights;

  const hasReviewerData = reviewer && reviewer.totalReviewsGiven >= 5;
  const hasReceivedData = received && received.totalReviewsReceived >= 5;
  const hasSignificanceData =
    sig && sig.significantCount + sig.notableCount + sig.routineCount >= 3;
  const hasTopicData = topics && topics.totalClassified >= 5;
  const hasHighlights = highlights.length > 0;

  const hasAnyData =
    hasReviewerData || hasReceivedData || hasSignificanceData || hasTopicData || hasHighlights;

  if (!hasAnyData && (!coverage || coverage.totalContributions === 0)) return null;

  const coveragePct =
    coverage && coverage.totalContributions > 0
      ? Math.round((coverage.enrichedContributions / coverage.totalContributions) * 100)
      : 0;

  return (
    <TooltipProvider>
      <Card>
        <CardHeader>
          <div className="flex items-center gap-2">
            <Sparkles className="size-4 text-muted-foreground" />
            <CardTitle>Insights</CardTitle>
            {coverage && (
              <Badge variant="secondary" className="ml-1 text-[10px]">
                {coveragePct}% enriched
              </Badge>
            )}
          </div>
          <CardDescription>
            AI-powered analysis of review quality, PR impact, and content
          </CardDescription>
        </CardHeader>

        <CardContent className="space-y-6">
          {!hasAnyData && (
            <div className="rounded-lg border-2 border-dashed p-8 text-center">
              <Sparkles className="mx-auto mb-2 size-8 text-muted-foreground" />
              <p className="mb-1 text-sm font-medium">Insights are building up</p>
              <p className="text-sm text-muted-foreground">
                {coveragePct}% of contributions enriched so far. Insights will appear here as the
                pipeline processes more data.
              </p>
              {coverage && <CoverageIndicator byType={coverage.byType} className="mt-4" />}
            </div>
          )}

          {hasAnyData && (
            <div className="grid gap-6 lg:grid-cols-2">
              {/* As a Reviewer — left column */}
              {hasReviewerData && reviewer && (
                <div className="space-y-4">
                  <h3 className="text-sm font-medium">As a reviewer</h3>
                  <p className="text-xs text-muted-foreground">How they review others' code</p>

                  <div className="grid grid-cols-2 gap-4">
                    <MetricValue
                      value={reviewer.avgDepth.toFixed(2)}
                      label="Avg depth"
                      description="Average review depth score (1–5 scale). Higher means more thorough reviews."
                      colorClass={depthColor(reviewer.avgDepth)}
                    />
                    <MetricValue
                      value={`${Math.round(reviewer.rubberStampPct)}%`}
                      label="Rubber-stamp"
                      description="Percentage of reviews scoring 1 (minimal/no feedback)."
                      colorClass={reviewer.rubberStampPct > 30 ? "text-red-600" : undefined}
                    />
                    <MetricValue
                      value={String(reviewer.totalReviewsGiven)}
                      label="Reviews given"
                      description="Number of reviews with depth enrichments in this period."
                    />
                  </div>

                  <DepthHistogram distribution={reviewer.depthDistribution} />

                  {reviewer.constructiveCount + reviewer.neutralCount + reviewer.criticalCount >
                    0 && (
                    <SentimentBar
                      constructive={reviewer.constructiveCount}
                      neutral={reviewer.neutralCount}
                      critical={reviewer.criticalCount}
                      hostile={0}
                    />
                  )}
                </div>
              )}

              {/* Reviews Received — right column */}
              {hasReceivedData && received && (
                <div className="space-y-4">
                  <h3 className="text-sm font-medium">Reviews received</h3>
                  <p className="text-xs text-muted-foreground">Quality of feedback on their PRs</p>

                  <div className="grid grid-cols-2 gap-4">
                    <MetricValue
                      value={received.avgDepthReceived.toFixed(2)}
                      label="Avg depth received"
                      description="Average depth of reviews on this person's PRs. Higher means they're getting more thorough feedback."
                      colorClass={depthColor(received.avgDepthReceived)}
                    />
                    <MetricValue
                      value={`${Math.round(received.deepReviewPct)}%`}
                      label="Deep reviews"
                      description="Percentage of reviews on their PRs scoring 4+ (thorough feedback)."
                      colorClass={received.deepReviewPct > 20 ? "text-emerald-600" : undefined}
                    />
                    <MetricValue
                      value={String(received.totalReviewsReceived)}
                      label="Reviews on their PRs"
                      description="Total reviews received on their pull requests in this period."
                    />
                  </div>
                </div>
              )}
            </div>
          )}

          {/* PR Impact — full width */}
          {hasSignificanceData && sig && (
            <div className="space-y-3">
              <h3 className="text-sm font-medium">PR Impact</h3>
              <SignificanceBreakdown
                significant={sig.significantCount}
                notable={sig.notableCount}
                routine={sig.routineCount}
              />
            </div>
          )}

          {/* Discourse Topics */}
          {hasTopicData && topics && (
            <div className="space-y-3">
              <h3 className="text-sm font-medium">Discourse Content</h3>
              <CategoryTags categories={topics.categories} />
            </div>
          )}

          {/* Coverage — always show when we have any data */}
          {hasAnyData && coverage && (
            <div className="space-y-3">
              <h3 className="text-sm font-medium">Coverage</h3>
              <CoverageIndicator byType={coverage.byType} />
            </div>
          )}

          {/* Highlights */}
          {hasHighlights && (
            <div className="space-y-3">
              <h3 className="text-sm font-medium">Highlights</h3>
              <div className="grid gap-3 lg:grid-cols-2">
                {highlights.slice(0, 3).map((item) => (
                  <NotableContributionCard key={item.contributionId} item={item} />
                ))}
              </div>
            </div>
          )}
        </CardContent>
      </Card>
    </TooltipProvider>
  );
};
