import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ChevronDown, ChevronRight, Loader2, MessageCircle } from "lucide-react";
import { useState } from "react";
import {
  Area,
  AreaChart,
  Bar,
  BarChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import type { Period, TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";
import type { TooltipContentProps } from "recharts/types/component/Tooltip";

import { useDiscourseActivity } from "@/views/teams/hooks/use-discourse-activity";

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

const cursorStyle = { fill: "hsl(var(--muted))", opacity: 0.5 };

export const DiscourseActivitySection = ({
  teamId,
  period,
  metrics,
}: {
  teamId: string;
  period: Period;
  metrics: TeamMetrics | undefined;
}): React.ReactElement | null => {
  const [open, setOpen] = useState(false);

  const discourseTopics = metrics?.discourseTopicsCreated ?? 0;
  const discoursePosts = metrics?.discoursePosts ?? 0;
  const hasDiscourse = discourseTopics > 0 || discoursePosts > 0;

  // Only fetch when section is expanded
  const { data, isLoading } = useDiscourseActivity(teamId, period);
  const enabled = open && hasDiscourse;

  if (!hasDiscourse) return null;

  const activityTrend = (data?.activityTrend ?? []).map((t) => ({
    date: t.date,
    topics: t.topics,
    posts: t.posts,
    likes: t.likes,
  }));

  const categories = data?.categoryDistribution ?? [];
  const contributors = data?.topContributors ?? [];

  return (
    <Collapsible open={open} onOpenChange={setOpen}>
      <Card>
        <CardHeader className="cursor-pointer" onClick={() => setOpen(!open)}>
          <CollapsibleTrigger
            render={<button type="button" className="flex w-full items-center gap-2 text-left" />}
          >
            {open ? <ChevronDown className="size-4" /> : <ChevronRight className="size-4" />}
            <MessageCircle className="size-4 text-muted-foreground" />
            <CardTitle>Discourse Activity</CardTitle>
            <Badge variant="secondary" className="ml-1">
              {discourseTopics + discoursePosts}
            </Badge>
          </CollapsibleTrigger>
        </CardHeader>
        <CollapsibleContent>
          <CardContent className="space-y-6 pt-0">
            {isLoading && enabled && (
              <div className="flex items-center justify-center p-8">
                <Loader2 className="size-5 animate-spin text-muted-foreground" />
              </div>
            )}

            {!isLoading && enabled && (
              <>
                {/* Activity trend chart */}
                {activityTrend.length > 1 && (
                  <div>
                    <h4 className="mb-3 text-sm font-medium">Activity Trend</h4>
                    <ResponsiveContainer width="100%" height={250}>
                      <AreaChart
                        data={activityTrend}
                        margin={{ top: 5, right: 30, left: 0, bottom: 5 }}
                      >
                        <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
                        <XAxis
                          dataKey="date"
                          tick={{ fontSize: 12 }}
                          className="fill-muted-foreground"
                        />
                        <YAxis className="fill-muted-foreground" />
                        <Tooltip content={ChartTooltip} cursor={cursorStyle} />
                        <Area
                          type="monotone"
                          dataKey="topics"
                          name="Topics"
                          stackId="1"
                          fill="hsl(var(--primary))"
                          stroke="hsl(var(--primary))"
                          fillOpacity={0.6}
                        />
                        <Area
                          type="monotone"
                          dataKey="posts"
                          name="Posts"
                          stackId="1"
                          fill="hsl(var(--muted-foreground))"
                          stroke="hsl(var(--muted-foreground))"
                          fillOpacity={0.4}
                        />
                        <Area
                          type="monotone"
                          dataKey="likes"
                          name="Likes"
                          stackId="1"
                          fill="hsl(var(--accent-foreground))"
                          stroke="hsl(var(--accent-foreground))"
                          fillOpacity={0.2}
                        />
                      </AreaChart>
                    </ResponsiveContainer>
                  </div>
                )}

                {/* Category distribution */}
                {categories.length > 0 && (
                  <div>
                    <h4 className="mb-3 text-sm font-medium">Category Distribution</h4>
                    <ResponsiveContainer
                      width="100%"
                      height={Math.min(categories.length * 32 + 40, 400)}
                    >
                      <BarChart
                        data={categories.map((c) => ({
                          name: c.category,
                          posts: c.posts,
                          topics: c.topics,
                        }))}
                        layout="vertical"
                        margin={{ top: 5, right: 30, left: 80, bottom: 5 }}
                      >
                        <CartesianGrid
                          strokeDasharray="3 3"
                          className="stroke-border"
                          horizontal={false}
                        />
                        <XAxis type="number" className="fill-muted-foreground" />
                        <YAxis
                          type="category"
                          dataKey="name"
                          tick={{ fontSize: 12 }}
                          className="fill-muted-foreground"
                          width={75}
                        />
                        <Tooltip content={ChartTooltip} cursor={cursorStyle} />
                        <Bar
                          dataKey="posts"
                          name="Posts"
                          fill="hsl(var(--primary))"
                          radius={[0, 4, 4, 0]}
                          stackId="cat"
                        />
                        <Bar
                          dataKey="topics"
                          name="Topics"
                          fill="hsl(var(--muted-foreground))"
                          radius={[0, 4, 4, 0]}
                          stackId="cat"
                        />
                      </BarChart>
                    </ResponsiveContainer>
                  </div>
                )}

                {/* Top contributors */}
                {contributors.length > 0 && (
                  <div>
                    <h4 className="mb-3 text-sm font-medium">Top Contributors</h4>
                    <div className="overflow-x-auto rounded-md border">
                      <Table>
                        <TableHeader>
                          <TableRow>
                            <TableHead>Name</TableHead>
                            <TableHead className="text-right">Topics</TableHead>
                            <TableHead className="text-right">Posts</TableHead>
                            <TableHead className="text-right">Likes Received</TableHead>
                            <TableHead className="text-right">Solved</TableHead>
                          </TableRow>
                        </TableHeader>
                        <TableBody>
                          {contributors.map((c) => (
                            <TableRow key={c.personId}>
                              <TableCell className="font-medium">{c.name}</TableCell>
                              <TableCell className="tabular-nums text-right">{c.topics}</TableCell>
                              <TableCell className="tabular-nums text-right">{c.posts}</TableCell>
                              <TableCell className="tabular-nums text-right">
                                {c.likesReceived || "\u2014"}
                              </TableCell>
                              <TableCell className="tabular-nums text-right">
                                {c.solved || "\u2014"}
                              </TableCell>
                            </TableRow>
                          ))}
                        </TableBody>
                      </Table>
                    </div>
                  </div>
                )}
              </>
            )}
          </CardContent>
        </CollapsibleContent>
      </Card>
    </Collapsible>
  );
};
