import { Badge } from "@/components/ui/badge";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Separator } from "@/components/ui/separator";
import { formatTimestamp } from "@/lib/format";
import { enrichmentTypeKey, enrichmentTypeLabel as etLabel } from "@/lib/proto-display";
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

const isSentimentValue = (v: unknown): v is SentimentValue => typeof v === "object" && v !== null && "sentiment" in v;

const isSignificanceValue = (v: unknown): v is SignificanceValue =>
  typeof v === "object" && v !== null && "significance" in v;

const isReviewDepthValue = (v: unknown): v is ReviewDepthValue => typeof v === "object" && v !== null && "score" in v;

const isTopicValue = (v: unknown): v is TopicValue => typeof v === "object" && v !== null && "primary_category" in v;

const badgeVariant = (enrichmentType: string, value: unknown): "default" | "secondary" | "destructive" | "outline" => {
  if (enrichmentType === "sentiment" && isSentimentValue(value)) {
    if (value.sentiment === "hostile") return "destructive";
    if (value.sentiment === "constructive") return "default";
    return "outline";
  }
  if (enrichmentType === "significance" && isSignificanceValue(value)) {
    if (value.significance === "significant") return "default";
    if (value.significance === "notable") return "secondary";
    return "outline";
  }
  return "secondary";
};

const badgeLabel = (enrichmentType: string, value: unknown): string => {
  if (enrichmentType === "review_depth" && isReviewDepthValue(value)) {
    return `${value.score}/5`;
  }
  if (enrichmentType === "sentiment" && isSentimentValue(value)) {
    return value.sentiment;
  }
  if (enrichmentType === "significance" && isSignificanceValue(value)) {
    return value.significance;
  }
  if (enrichmentType === "topic" && isTopicValue(value)) {
    return value.primary_category;
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
  const parsed: Record<string, unknown> = JSON.parse(enrichment.valueJson || "{}");
  const etKey = enrichmentTypeKey(enrichment.enrichmentType);
  const icon = ENRICHMENT_ICONS[etKey] ?? <Brain className="size-3" />;
  const label = ENRICHMENT_LABELS[etKey] ?? etLabel(enrichment.enrichmentType);
  const displayLabel = badgeLabel(etKey, parsed);
  const variant = badgeVariant(etKey, parsed);
  const confidence = typeof parsed.confidence === "number" ? parsed.confidence : 0;
  const rationale = typeof parsed.rationale === "string" ? parsed.rationale : "";

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
              <p className="max-h-24 overflow-y-auto text-xs text-muted-foreground">{enrichment.inputPreview}</p>
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
