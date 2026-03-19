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
import { Link } from "react-router";

import type { TeamInsights } from "@ps/api/gen/prism/v1/insights_pb";

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

export const TeamInsightsSection = ({
  insights,
  isLoading,
}: {
  insights: TeamInsights | undefined;
  isLoading: boolean;
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

  if (!insights) return null;

  const coverage = insights.coverage;
  const rq = insights.reviewQuality;
  const sig = insights.prSignificance;
  const topics = insights.discourseTopics;
  const notable = insights.notableItems;
  const dbs = insights.depthBySignificance;

  const hasReviewData = rq && rq.totalReviews >= 10;
  const hasSignificanceData =
    sig && sig.significantCount + sig.notableCount + sig.routineCount >= 5;
  const hasTopicData = topics && topics.totalClassified >= 5;
  const hasNotable = notable.length > 0;
  const hasDbs =
    dbs && dbs.significantReviewCount + dbs.notableReviewCount + dbs.routineReviewCount >= 10;

  const hasAnyData = hasReviewData || hasSignificanceData || hasTopicData || hasNotable;

  if (!hasAnyData && coverage && coverage.enrichedContributions === 0) return null;

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
            <div className="grid gap-6 lg:grid-cols-3">
              {/* Review Quality — 2/3 width */}
              {hasReviewData && rq && (
                <div className="space-y-4 lg:col-span-2">
                  <h3 className="text-sm font-medium">Review Quality</h3>

                  <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
                    <MetricValue
                      value={rq.avgDepth.toFixed(2)}
                      label="Avg depth"
                      description="Average review depth score (1–5 scale). Higher means more thorough reviews."
                      colorClass={depthColor(rq.avgDepth)}
                    />
                    <MetricValue
                      value={`${Math.round(rq.rubberStampPct)}%`}
                      label="Rubber-stamp"
                      description="Percentage of reviews scoring 1 (minimal/no feedback)."
                      colorClass={rq.rubberStampPct > 30 ? "text-red-600" : undefined}
                    />
                    <MetricValue
                      value={`${Math.round(rq.deepReviewPct)}%`}
                      label="Deep reviews"
                      description="Percentage of reviews scoring 4 or 5 (thorough feedback)."
                      colorClass={rq.deepReviewPct > 20 ? "text-emerald-600" : undefined}
                    />
                    <MetricValue
                      value={String(rq.totalReviews)}
                      label="Total reviews"
                      description="Number of reviews with depth enrichments in this period."
                    />
                  </div>

                  <DepthHistogram distribution={rq.depthDistribution} />

                  {rq.constructiveCount + rq.neutralCount + rq.criticalCount + rq.hostileCount >
                    0 && (
                    <SentimentBar
                      constructive={rq.constructiveCount}
                      neutral={rq.neutralCount}
                      critical={rq.criticalCount}
                      hostile={rq.hostileCount}
                    />
                  )}

                  {/* Top reviewers */}
                  {rq.topReviewers.length > 0 && (
                    <div>
                      <h4 className="mb-2 text-xs font-medium text-muted-foreground">
                        Top reviewers by depth
                      </h4>
                      <div className="space-y-1">
                        {rq.topReviewers.map((r) => (
                          <div
                            key={r.personId}
                            className="flex items-center justify-between rounded px-2 py-1 text-sm hover:bg-muted/50"
                          >
                            <Link
                              to={`/people/${r.personId}`}
                              className="underline-offset-4 hover:underline"
                            >
                              {r.personName}
                            </Link>
                            <div className="flex items-center gap-3 text-xs text-muted-foreground">
                              <span className="tabular-nums">{r.reviewCount} reviews</span>
                              <span
                                className={`tabular-nums font-medium ${depthColor(r.avgDepth)}`}
                              >
                                {r.avgDepth.toFixed(2)}
                              </span>
                            </div>
                          </div>
                        ))}
                      </div>
                    </div>
                  )}
                </div>
              )}

              {/* Right column: PR Impact + Discourse */}
              <div className="space-y-6">
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

                {hasTopicData && topics && (
                  <div className="space-y-3">
                    <h3 className="text-sm font-medium">Discourse Content</h3>
                    <CategoryTags categories={topics.categories} />
                  </div>
                )}

                {coverage && (
                  <div className="space-y-3">
                    <h3 className="text-sm font-medium">Coverage</h3>
                    <CoverageIndicator byType={coverage.byType} />
                  </div>
                )}
              </div>
            </div>
          )}

          {/* Depth × Significance cross-reference */}
          {hasDbs && dbs && (
            <div className="space-y-3">
              <h3 className="text-sm font-medium">Review depth by PR significance</h3>
              <div className="grid grid-cols-3 gap-4 text-center">
                {[
                  {
                    label: "Significant",
                    depth: dbs.avgDepthSignificant,
                    count: dbs.significantReviewCount,
                  },
                  { label: "Notable", depth: dbs.avgDepthNotable, count: dbs.notableReviewCount },
                  { label: "Routine", depth: dbs.avgDepthRoutine, count: dbs.routineReviewCount },
                ].map((row) => (
                  <div key={row.label} className="rounded-lg border p-3">
                    <p className="text-xs text-muted-foreground">{row.label}</p>
                    <p className={`text-lg font-semibold tabular-nums ${depthColor(row.depth)}`}>
                      {row.count > 0 ? row.depth.toFixed(2) : "\u2014"}
                    </p>
                    <p className="text-[10px] tabular-nums text-muted-foreground">
                      {row.count} reviews
                    </p>
                  </div>
                ))}
              </div>
            </div>
          )}

          {/* Notable Contributions */}
          {hasNotable && (
            <div className="space-y-3">
              <h3 className="text-sm font-medium">Notable contributions</h3>
              <div className="grid gap-3 lg:grid-cols-2">
                {notable.slice(0, 4).map((item) => (
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
