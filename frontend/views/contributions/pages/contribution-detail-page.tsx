import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Button } from "@/components/ui/button";
import type { Enrichment } from "@ps/api/gen/prism/v1/reasoning_pb";
import { useEnrichments } from "@/views/admin/hooks/use-enrichment";
import { RelatedItems } from "@/views/contributions/components/related-items";
import { ArrowLeft } from "lucide-react";
import { useParams, useNavigate } from "react-router";

const enrichmentLabel = (type: string, valueJson: string): string => {
  try {
    const v = JSON.parse(valueJson);
    switch (type) {
      case "significance":
        return v.label ?? type;
      case "review_depth":
        return `Depth: ${v.score}/5`;
      case "sentiment":
        return v.sentiment ?? v.label ?? type;
      case "topic":
        return v.primary_category ?? type;
      default:
        return type;
    }
  } catch {
    return type;
  }
};

const enrichmentVariant = (type: string): "default" | "secondary" | "outline" => {
  switch (type) {
    case "significance":
      return "default";
    case "review_depth":
      return "secondary";
    default:
      return "outline";
  }
};

const EnrichmentBadges = ({
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
        <Badge key={e.id} variant={enrichmentVariant(e.enrichmentType)} className="text-xs">
          {enrichmentLabel(e.enrichmentType, e.valueJson)}
        </Badge>
      ))}
    </div>
  );
};

const ContributionDetailPage = (): React.ReactElement | null => {
  const { contributionId } = useParams<{ contributionId: string }>();
  const navigate = useNavigate();
  const { data: enrichments, isLoading: enrichmentsLoading } = useEnrichments(contributionId ?? "");

  if (!contributionId) return null;

  return (
    <>
      <PageHeader
        title="Contribution Detail"
        actions={
          <Button variant="ghost" size="sm" onClick={() => navigate(-1)}>
            <ArrowLeft className="mr-1 size-4" />
            Back
          </Button>
        }
      />
      <div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">
        {/* Enrichments */}
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="text-sm">Enrichments</CardTitle>
          </CardHeader>
          <CardContent>
            <EnrichmentBadges enrichments={enrichments} isLoading={enrichmentsLoading} />
          </CardContent>
        </Card>

        {/* Similar Contributions */}
        <RelatedItems contributionId={contributionId} />
      </div>
    </>
  );
};

export default ContributionDetailPage;
