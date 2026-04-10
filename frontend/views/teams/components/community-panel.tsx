import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { instanceLabel } from "@/lib/format-metrics";
import { ArrowRight, Info, MessageCircle } from "lucide-react";
import { useMemo } from "react";

import type { TeamMetrics } from "@ps/api/gen/canonical/prism/v1/metrics_pb";

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
    <button type="button" onClick={onClick} disabled={!onClick} className="group text-left disabled:cursor-default">
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

const buildSummary = (metrics: TeamMetrics, instanceCount: number): string => {
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
  const suffix = instanceCount > 1 ? ` across ${instanceCount} Discourse instances.` : " across Discourse.";
  return parts.join(" and ") + suffix;
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

  // Group instances by display label to merge duplicates (e.g. "canonical-discourse" and
  // "Canonical Discourse" both map to the same label via instanceLabel).
  const byInstance = metrics.discourseByInstance;
  const groupedInstances = useMemo(() => {
    const raw = byInstance ?? [];
    const map = new Map<string, { topics: number; posts: number; likes: number }>();
    for (const inst of raw) {
      const label = instanceLabel(inst.instance);
      const existing = map.get(label) ?? { topics: 0, posts: 0, likes: 0 };
      existing.topics += inst.topicsCreated;
      existing.posts += inst.posts;
      existing.likes += inst.likesGiven;
      map.set(label, existing);
    }
    return [...map.entries()]
      .map(([label, counts]) => ({ label, ...counts }))
      .toSorted((a, b) => b.topics + b.posts - (a.topics + a.posts));
  }, [byInstance]);
  const hasMultipleInstances = groupedInstances.length > 1;

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
          {/* Aggregate totals */}
          <div className="grid grid-cols-3 gap-4">
            <MetricValue
              value={String(discourseTopics)}
              label="Topics Created"
              description="New Discourse topics started by team members."
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
              onClick={onScrollToDiscourse}
            />
          </div>

          {/* Per-instance breakdown as a compact table */}
          {hasMultipleInstances && (
            <div className="overflow-x-auto rounded-md border">
              <table className="w-full text-xs">
                <thead>
                  <tr className="border-b bg-muted/50">
                    <th className="px-3 py-1.5 text-left font-medium text-muted-foreground">Instance</th>
                    <th className="px-3 py-1.5 text-right font-medium text-muted-foreground">Topics</th>
                    <th className="px-3 py-1.5 text-right font-medium text-muted-foreground">Posts</th>
                    <th className="px-3 py-1.5 text-right font-medium text-muted-foreground">Likes</th>
                  </tr>
                </thead>
                <tbody>
                  {groupedInstances.map((inst) => (
                    <tr key={inst.label} className="border-b last:border-0">
                      <td className="px-3 py-1.5 font-medium">{inst.label}</td>
                      <td className="px-3 py-1.5 text-right tabular-nums">{inst.topics || "\u2014"}</td>
                      <td className="px-3 py-1.5 text-right tabular-nums">{inst.posts || "\u2014"}</td>
                      <td className="px-3 py-1.5 text-right tabular-nums">{inst.likes || "\u2014"}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}

          <p className="flex items-center gap-1.5 text-sm text-muted-foreground">
            <ArrowRight className="size-3.5 shrink-0" />
            {buildSummary(metrics, groupedInstances.length)}
          </p>
        </CardContent>
      </Card>
    </TooltipProvider>
  );
};
