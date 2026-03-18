import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { ArrowRight, Info, MessageCircle } from "lucide-react";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";

const MetricValue = ({
  value,
  label,
  description,
  secondary,
  onClick,
}: {
  value: string;
  label: string;
  description: string;
  secondary?: string;
  onClick?: () => void;
}): React.ReactElement => (
  <div className="min-w-0 flex-1">
    <button
      type="button"
      onClick={onClick}
      disabled={!onClick}
      className="group text-left disabled:cursor-default"
    >
      <span className="text-2xl font-semibold tabular-nums group-enabled:underline-offset-4 group-enabled:hover:underline">
        {value}
      </span>
    </button>
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
    {secondary && <p className="mt-0.5 text-[10px] text-muted-foreground/70">{secondary}</p>}
  </div>
);

const buildSummary = (metrics: TeamMetrics): string => {
  const topics = metrics.discourseTopicsCreated;
  const posts = metrics.discoursePosts;
  const parts: string[] = [];
  if (topics > 0) {
    parts.push(`${topics} new topic${topics !== 1 ? "s" : ""}`);
  }
  if (posts > 0) {
    parts.push(`${posts} post${posts !== 1 ? "s" : ""}`);
  }
  if (parts.length === 0) return "No Discourse activity in this period.";
  return parts.join(" and ") + " across Discourse.";
};

export const CommunityPanel = ({
  metrics,
  onScrollToDiscourse,
}: {
  metrics: TeamMetrics | undefined;
  onScrollToDiscourse?: () => void;
}): React.ReactElement | null => {
  if (!metrics) return null;

  const discourseTopics = metrics.discourseTopicsCreated;
  const discoursePosts = metrics.discoursePosts;
  const discourseReplies = metrics.discourseReplies;
  const discourseLikesGiven = metrics.discourseLikesGiven;
  const hasDiscourse = discourseTopics > 0 || discoursePosts > 0;

  if (!hasDiscourse) return null;

  // Build per-instance secondary text
  const instanceBreakdown = (
    field: "topicsCreated" | "posts" | "likesGiven",
  ): string | undefined => {
    const instances = metrics.discourseByInstance ?? [];
    if (instances.length <= 1) return undefined;
    return instances
      .filter((i) => i[field] > 0)
      .map((i) => `${i.instance}: ${i[field]}`)
      .join(" \u00b7 ");
  };

  return (
    <TooltipProvider>
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center gap-2">
            <MessageCircle className="size-4 text-muted-foreground" />
            <CardTitle>Community</CardTitle>
          </div>
          <CardDescription>Discourse participation and engagement.</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid grid-cols-3 gap-4">
            <MetricValue
              value={String(discourseTopics)}
              label="Topics Created"
              description="New Discourse topics started by team members."
              secondary={instanceBreakdown("topicsCreated")}
              onClick={onScrollToDiscourse}
            />
            <MetricValue
              value={String(discoursePosts)}
              label="Posts & Replies"
              description="Total posts on Discourse, including replies."
              secondary={discourseReplies > 0 ? `${discourseReplies} replies` : undefined}
              onClick={onScrollToDiscourse}
            />
            <MetricValue
              value={String(discourseLikesGiven)}
              label="Likes Given"
              description="Likes given by team members on Discourse."
              secondary={instanceBreakdown("likesGiven")}
              onClick={onScrollToDiscourse}
            />
          </div>

          <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
            <ArrowRight className="size-3.5 shrink-0" />
            {buildSummary(metrics)}
          </p>
        </CardContent>
      </Card>
    </TooltipProvider>
  );
};
