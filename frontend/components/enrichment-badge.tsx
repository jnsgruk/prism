import { Badge } from "@/components/ui/badge";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Separator } from "@/components/ui/separator";
import { formatTimestamp } from "@/lib/format";
import { Brain, MessageCircle, Sparkles, Star, Tag } from "lucide-react";

import type { Enrichment } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

// ---------------------------------------------------------------------------
// Types parsed from the enrichment value_json
// ---------------------------------------------------------------------------

interface ReviewDepthValue {
  score: number;
  rationale: string;
  confidence: number;
}

interface SentimentValue {
  sentiment: string;
  rationale: string;
  confidence: number;
}

interface SignificanceValue {
  significance: string;
  rationale: string;
  confidence: number;
}

interface TopicValue {
  primary_category: string;
  secondary_category?: string;
  rationale: string;
  confidence: number;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

const ENRICHMENT_ICONS: Record<string, React.ReactNode> = {
  review_depth: <Star className="size-3" />,
  sentiment: <MessageCircle className="size-3" />,
  significance: <Sparkles className="size-3" />,
  topic: <Tag className="size-3" />,
};

const ENRICHMENT_LABELS: Record<string, string> = {
  review_depth: "Depth",
  sentiment: "Sentiment",
  significance: "Significance",
  topic: "Topic",
};

const badgeVariant = (
  enrichmentType: string,
  value: unknown,
): "default" | "secondary" | "destructive" | "outline" => {
  if (enrichmentType === "sentiment") {
    const v = value as SentimentValue;
    if (v.sentiment === "hostile") return "destructive";
    if (v.sentiment === "constructive") return "default";
    return "outline";
  }
  if (enrichmentType === "significance") {
    const v = value as SignificanceValue;
    if (v.significance === "significant") return "default";
    if (v.significance === "notable") return "secondary";
    return "outline";
  }
  return "secondary";
};

const badgeLabel = (enrichmentType: string, value: unknown): string => {
  if (enrichmentType === "review_depth") {
    const v = value as ReviewDepthValue;
    return `${v.score}/5`;
  }
  if (enrichmentType === "sentiment") {
    const v = value as SentimentValue;
    return v.sentiment;
  }
  if (enrichmentType === "significance") {
    const v = value as SignificanceValue;
    return v.significance;
  }
  if (enrichmentType === "topic") {
    const v = value as TopicValue;
    return v.primary_category;
  }
  return enrichmentType;
};

// ---------------------------------------------------------------------------
// Components
// ---------------------------------------------------------------------------

interface EnrichmentBadgeProps {
  enrichment: Enrichment;
}

/** A clickable badge that shows enrichment score/label, with a popover for provenance. */
const EnrichmentBadge = ({ enrichment }: EnrichmentBadgeProps): React.ReactElement => {
  const parsed = JSON.parse(enrichment.valueJson || "{}") as Record<string, unknown>;
  const icon = ENRICHMENT_ICONS[enrichment.enrichmentType] ?? <Brain className="size-3" />;
  const label = ENRICHMENT_LABELS[enrichment.enrichmentType] ?? enrichment.enrichmentType;
  const displayLabel = badgeLabel(enrichment.enrichmentType, parsed);
  const variant = badgeVariant(enrichment.enrichmentType, parsed);
  const confidence = (parsed.confidence as number) ?? 0;
  const rationale = (parsed.rationale as string) ?? "";

  return (
    <Popover>
      <PopoverTrigger className="cursor-pointer">
        <Badge variant={variant} className="gap-1 text-[10px] uppercase">
          {icon}
          {label}: {displayLabel}
        </Badge>
      </PopoverTrigger>
      <PopoverContent className="w-80 space-y-3 text-sm" align="start">
        <div className="space-y-1">
          <p className="font-medium">{label}</p>
          <p className="text-muted-foreground">{rationale}</p>
        </div>

        <Separator />

        <div className="space-y-1.5 text-xs text-muted-foreground">
          <div className="flex justify-between">
            <span>Confidence</span>
            <span className="tabular-nums">{Math.round(confidence * 100)}%</span>
          </div>
          <div className="flex justify-between">
            <span>Model</span>
            <span className="font-mono">{enrichment.modelName}</span>
          </div>
          <div className="flex justify-between">
            <span>Created</span>
            <span>{formatTimestamp(enrichment.createdAt)}</span>
          </div>
          {enrichment.inputHash && (
            <div className="flex justify-between">
              <span>Input Hash</span>
              <span className="max-w-[160px] truncate font-mono">{enrichment.inputHash}</span>
            </div>
          )}
        </div>

        {enrichment.inputPreview && (
          <>
            <Separator />
            <div className="space-y-1">
              <p className="text-xs font-medium text-muted-foreground">Input Preview</p>
              <p className="max-h-24 overflow-y-auto text-xs text-muted-foreground">
                {enrichment.inputPreview}
              </p>
            </div>
          </>
        )}
      </PopoverContent>
    </Popover>
  );
};

interface EnrichmentBadgeListProps {
  enrichments: Enrichment[];
}

/** Renders a row of enrichment badges for a contribution. */
const EnrichmentBadgeList = ({ enrichments }: EnrichmentBadgeListProps): React.ReactElement => {
  if (enrichments.length === 0) return <></>;

  return (
    <div className="flex flex-wrap gap-1">
      {enrichments.map((e) => (
        <EnrichmentBadge key={e.id} enrichment={e} />
      ))}
    </div>
  );
};

export { EnrichmentBadge, EnrichmentBadgeList };
