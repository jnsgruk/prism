import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { useContribution } from "@/lib/hooks/use-metrics";
import {
  ContributionState,
  ContributionType,
  EnrichmentType,
} from "@ps/api/gen/canonical/prism/v1/common_pb";
import {
  contributionStateLabel,
  contributionTypeLabel as ctLabel,
  enrichmentTypeKey,
  enrichmentTypeLabel as etDisplayLabel,
  platformLabel,
} from "@/lib/proto-display";
import type { Enrichment } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { useEnrichments } from "@/lib/hooks/use-enrichment";
import { RelatedItems } from "@/views/contributions/components/related-items";
import {
  ArrowLeft,
  Brain,
  Calendar,
  ExternalLink,
  FileCode,
  GitBranch,
  Loader2,
  Tag,
  User,
} from "lucide-react";
import { Link, useParams, useNavigate } from "react-router";
import type { Timestamp } from "@bufbuild/protobuf/wkt";

// ---------------------------------------------------------------------------
// Formatting helpers
// ---------------------------------------------------------------------------

const formatTimestamp = (ts?: Timestamp): string => {
  if (!ts) return "\u2014";
  const date = new Date(Number(ts.seconds) * 1000);
  return (
    date.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
    " " +
    date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false })
  );
};

const stateBadgeVariant = (
  state: ContributionState,
): "default" | "secondary" | "destructive" | "outline" => {
  switch (state) {
    case ContributionState.MERGED:
    case ContributionState.APPROVED:
      return "default";
    case ContributionState.OPEN:
      return "outline";
    case ContributionState.CLOSED:
    case ContributionState.CHANGES_REQUESTED:
      return "destructive";
    default:
      return "secondary";
  }
};

// ---------------------------------------------------------------------------
// Enrichment helpers
// ---------------------------------------------------------------------------

const enrichmentLabel = (type: EnrichmentType, valueJson: string): string => {
  try {
    const v = JSON.parse(valueJson);
    switch (type) {
      case EnrichmentType.SIGNIFICANCE:
        return v.label ?? enrichmentTypeKey(type);
      case EnrichmentType.REVIEW_DEPTH:
        return `Depth: ${v.score}/5`;
      case EnrichmentType.SENTIMENT:
        return v.sentiment ?? v.label ?? enrichmentTypeKey(type);
      case EnrichmentType.TOPIC:
        return v.primary_category ?? enrichmentTypeKey(type);
      default:
        return enrichmentTypeKey(type);
    }
  } catch {
    return enrichmentTypeKey(type);
  }
};

const enrichmentVariant = (type: EnrichmentType): "default" | "secondary" | "outline" => {
  switch (type) {
    case EnrichmentType.SIGNIFICANCE:
      return "default";
    case EnrichmentType.REVIEW_DEPTH:
      return "secondary";
    default:
      return "outline";
  }
};

const enrichmentRationale = (valueJson: string): string | null => {
  try {
    const v = JSON.parse(valueJson);
    return v.rationale ?? v.reasoning ?? null;
  } catch {
    return null;
  }
};

// ---------------------------------------------------------------------------
// Sub-components
// ---------------------------------------------------------------------------

const MetadataRow = ({
  icon: Icon,
  label,
  children,
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  children: React.ReactNode;
}): React.ReactElement => (
  <div className="flex items-start gap-3">
    <Icon className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
    <div className="min-w-0">
      <p className="text-xs text-muted-foreground">{label}</p>
      <div className="text-sm">{children}</div>
    </div>
  </div>
);

const EnrichmentBadge = ({ enrichment }: { enrichment: Enrichment }): React.ReactElement => {
  const rationale = enrichmentRationale(enrichment.valueJson);
  const badge = (
    <Badge
      variant={enrichmentVariant(enrichment.enrichmentType)}
      className="cursor-default text-xs"
    >
      {enrichmentLabel(enrichment.enrichmentType, enrichment.valueJson)}
    </Badge>
  );

  if (!rationale && !enrichment.modelName) return badge;

  return (
    <Popover>
      <PopoverTrigger render={<button type="button" className="cursor-pointer" />}>
        {badge}
      </PopoverTrigger>
      <PopoverContent className="w-80 space-y-2 text-sm" side="bottom" align="start">
        <p className="font-medium">{etDisplayLabel(enrichment.enrichmentType)}</p>
        {rationale && <p className="text-muted-foreground">{rationale}</p>}
        <div className="flex items-center gap-3 text-xs text-muted-foreground">
          {enrichment.modelName && <span>Model: {enrichment.modelName}</span>}
          {enrichment.confidence != null && enrichment.confidence > 0 && (
            <span>Confidence: {Math.round(enrichment.confidence * 100)}%</span>
          )}
        </div>
      </PopoverContent>
    </Popover>
  );
};

const EnrichmentsSection = ({
  enrichments,
  isLoading,
}: {
  enrichments: Enrichment[] | undefined;
  isLoading: boolean;
}): React.ReactElement => {
  if (isLoading) return <Skeleton className="h-8 w-48" />;
  if (!enrichments || enrichments.length === 0) {
    return <p className="text-sm text-muted-foreground">No enrichments yet</p>;
  }
  return (
    <div className="flex flex-wrap gap-2">
      {enrichments.map((e) => (
        <EnrichmentBadge key={e.id} enrichment={e} />
      ))}
    </div>
  );
};

const ChangeStats = ({
  additions,
  deletions,
  changedFiles,
}: {
  additions: number;
  deletions: number;
  changedFiles: number;
}): React.ReactElement | null => {
  if (additions === 0 && deletions === 0) return null;
  return (
    <div className="flex items-center gap-3 text-sm">
      <span className="text-green-600">+{additions}</span>
      <span className="text-red-600">&minus;{deletions}</span>
      {changedFiles > 0 && <span className="text-muted-foreground">{changedFiles} files</span>}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Main page
// ---------------------------------------------------------------------------

const ContributionDetailPage = (): React.ReactElement | null => {
  const { contributionId } = useParams<{ contributionId: string }>();
  const navigate = useNavigate();
  const { data: contribution, isLoading } = useContribution(contributionId ?? "");
  const { data: enrichments, isLoading: enrichmentsLoading } = useEnrichments(contributionId ?? "");

  if (!contributionId) return null;

  if (isLoading) {
    return (
      <>
        <PageHeader title="Contribution" />
        <div className="flex min-w-0 flex-1 items-center justify-center">
          <Loader2 className="size-6 animate-spin text-muted-foreground" />
        </div>
      </>
    );
  }

  if (!contribution) {
    return (
      <>
        <PageHeader
          title="Contribution"
          actions={
            <Button variant="ghost" size="sm" onClick={() => navigate(-1)}>
              <ArrowLeft className="mr-1 size-4" />
              Back
            </Button>
          }
        />
        <div className="flex min-w-0 flex-1 flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
          <p className="mb-1 font-medium">Not found</p>
          <p className="text-sm text-muted-foreground">This contribution does not exist.</p>
        </div>
      </>
    );
  }

  const isPR = contribution.contributionType === ContributionType.PULL_REQUEST;
  const isReview = contribution.contributionType === ContributionType.PR_REVIEW;

  return (
    <>
      <PageHeader
        title={contribution.title || "Untitled"}
        description={`${platformLabel(contribution.platform)} \u00b7 ${ctLabel(contribution.contributionType)}`}
        actions={
          <div className="flex items-center gap-2">
            {contribution.url && (
              <Button
                variant="outline"
                size="sm"
                render={<a href={contribution.url} target="_blank" rel="noopener noreferrer" />}
              >
                <ExternalLink className="mr-1 size-3.5" />
                Open in {platformLabel(contribution.platform)}
              </Button>
            )}
            <Button variant="ghost" size="sm" onClick={() => navigate(-1)}>
              <ArrowLeft className="mr-1 size-4" />
              Back
            </Button>
          </div>
        }
      />
      <div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">
        {/* Summary card — full width */}
        <Card>
          <CardContent className="pt-6">
            <div className="flex flex-wrap items-start gap-x-8 gap-y-4">
              {/* State */}
              {contribution.state && (
                <div className="flex items-center gap-2">
                  <Badge
                    variant={stateBadgeVariant(contribution.state)}
                    className="text-[10px] uppercase"
                  >
                    {contributionStateLabel(contribution.state)}
                  </Badge>
                  {contribution.draft && (
                    <Badge variant="outline" className="text-[10px] uppercase">
                      Draft
                    </Badge>
                  )}
                </div>
              )}

              {/* Author */}
              {contribution.personName && (
                <MetadataRow icon={User} label="Author">
                  {contribution.personId ? (
                    <Link
                      to={`/people/${contribution.personId}`}
                      className="text-foreground hover:underline"
                    >
                      {contribution.personName}
                    </Link>
                  ) : (
                    contribution.personName
                  )}
                </MetadataRow>
              )}

              {/* Repo / Category */}
              {contribution.repo && (
                <MetadataRow icon={FileCode} label="Repository">
                  {contribution.repo}
                </MetadataRow>
              )}
              {contribution.category && (
                <MetadataRow icon={Tag} label="Category">
                  {contribution.category}
                </MetadataRow>
              )}

              {/* Branch */}
              {isPR && contribution.headRef && (
                <MetadataRow icon={GitBranch} label="Branch">
                  <span className="font-mono text-xs">
                    {contribution.headRef}
                    {contribution.baseRef && (
                      <span className="text-muted-foreground">
                        {" \u2192 "}
                        {contribution.baseRef}
                      </span>
                    )}
                  </span>
                </MetadataRow>
              )}

              {/* Dates */}
              <MetadataRow icon={Calendar} label="Created">
                {formatTimestamp(contribution.createdAt)}
              </MetadataRow>
              {contribution.closedAt && (
                <MetadataRow icon={Calendar} label="Closed">
                  {formatTimestamp(contribution.closedAt)}
                </MetadataRow>
              )}

              {/* PR stats */}
              {isPR && (contribution.additions > 0 || contribution.deletions > 0) && (
                <div>
                  <p className="text-xs text-muted-foreground">Changes</p>
                  <ChangeStats
                    additions={contribution.additions}
                    deletions={contribution.deletions}
                    changedFiles={contribution.changedFiles}
                  />
                </div>
              )}

              {/* Reviews */}
              {isPR && contribution.reviewCount > 0 && (
                <div>
                  <p className="text-xs text-muted-foreground">Reviews</p>
                  <p className="text-sm">
                    {contribution.reviewCount}
                    {contribution.reviewHours > 0 && (
                      <span className="text-muted-foreground">
                        {" \u00b7 "}
                        {contribution.reviewHours.toFixed(1)}h to first
                      </span>
                    )}
                  </p>
                </div>
              )}

              {/* Review turnaround */}
              {isReview && contribution.reviewHours > 0 && (
                <div>
                  <p className="text-xs text-muted-foreground">Review time</p>
                  <p className="text-sm">{contribution.reviewHours.toFixed(1)}h</p>
                </div>
              )}
            </div>

            {/* Labels */}
            {contribution.labels.length > 0 && (
              <>
                <Separator className="my-4" />
                <div className="flex flex-wrap gap-1">
                  {contribution.labels.map((label) => (
                    <Badge key={label} variant="outline" className="text-xs">
                      {label}
                    </Badge>
                  ))}
                </div>
              </>
            )}
          </CardContent>
        </Card>

        {/* Content (description / body) */}
        {contribution.content && (
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="text-sm">Description</CardTitle>
            </CardHeader>
            <CardContent>
              <div className="max-h-[60vh] overflow-y-auto">
                <pre className="whitespace-pre-wrap break-words text-sm text-muted-foreground">
                  {contribution.content}
                </pre>
              </div>
            </CardContent>
          </Card>
        )}

        {/* AI Enrichments + Similar — side by side on large screens */}
        <div className="grid gap-6 lg:grid-cols-2">
          <Card>
            <CardHeader className="pb-3">
              <CardTitle className="flex items-center gap-2 text-sm">
                <Brain className="size-4" /> AI Enrichments
              </CardTitle>
            </CardHeader>
            <CardContent>
              <EnrichmentsSection enrichments={enrichments} isLoading={enrichmentsLoading} />
            </CardContent>
          </Card>

          <RelatedItems contributionId={contributionId} currentPlatform={contribution.platform} />
        </div>
      </div>
    </>
  );
};

export default ContributionDetailPage;
