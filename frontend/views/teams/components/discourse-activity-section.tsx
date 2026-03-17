import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { ChevronDown, ChevronRight, MessageCircle, Search } from "lucide-react";
import { useRef, useMemo, useState } from "react";
import type { ColumnDef, SortingState } from "@tanstack/react-table";
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

import type { Period, TeamMetrics, TopContributor } from "@ps/api/gen/prism/v1/metrics_pb";
import type { TooltipContentProps } from "recharts/types/component/Tooltip";

import { useListSources } from "@/lib/hooks/use-config";
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

// --- Contributor table columns ---

const nameColumn: ColumnDef<TopContributor, unknown> = {
  id: "name",
  accessorKey: "name",
  header: "Name",
  cell: ({ row }) => <span className="font-medium">{row.original.name}</span>,
  enableSorting: true,
};

const topicsColumn: ColumnDef<TopContributor, unknown> = {
  id: "topics",
  accessorKey: "topics",
  header: "Topics",
  cell: ({ row }) => <span className="tabular-nums text-right">{row.original.topics}</span>,
  enableSorting: true,
};

const postsColumn: ColumnDef<TopContributor, unknown> = {
  id: "posts",
  accessorKey: "posts",
  header: "Posts",
  cell: ({ row }) => <span className="tabular-nums text-right">{row.original.posts}</span>,
  enableSorting: true,
};

const likesColumn: ColumnDef<TopContributor, unknown> = {
  id: "likes_received",
  accessorKey: "likesReceived",
  header: "Likes Received",
  cell: ({ row }) => (
    <span className="tabular-nums text-right">{row.original.likesReceived || "\u2014"}</span>
  ),
  enableSorting: true,
};

const contributorColumns: ColumnDef<TopContributor, unknown>[] = [
  nameColumn,
  topicsColumn,
  postsColumn,
  likesColumn,
];

// --- Sorting helper ---

const sortContributors = (data: TopContributor[], sorting: SortingState): TopContributor[] => {
  const col = sorting[0];
  if (!col) return data;
  const { id, desc } = col;
  const sorted = data.toSorted((a, b) => {
    let cmp = 0;
    switch (id) {
      case "name":
        cmp = a.name.localeCompare(b.name);
        break;
      case "topics":
        cmp = a.topics - b.topics;
        break;
      case "posts":
        cmp = a.posts - b.posts;
        break;
      case "likes_received":
        cmp = a.likesReceived - b.likesReceived;
        break;
    }
    return desc ? -cmp : cmp;
  });
  return sorted;
};

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
  const [instanceFilter, setInstanceFilter] = useState("all");

  // Contributor table state
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [pageSize, setPageSize] = useState(10);
  const [pageIndex, setPageIndex] = useState(0);
  const [sorting, setSorting] = useState<SortingState>([]);

  const searchTimerRef = useRef<ReturnType<typeof setTimeout>>(undefined);
  const handleSearchChange = (value: string): void => {
    setSearch(value);
    clearTimeout(searchTimerRef.current);
    searchTimerRef.current = setTimeout(() => {
      setDebouncedSearch(value);
      setPageIndex(0);
    }, 300);
  };

  const discourseTopics = metrics?.discourseTopicsCreated ?? 0;
  const discoursePosts = metrics?.discoursePosts ?? 0;
  const hasDiscourse = discourseTopics > 0 || discoursePosts > 0;

  // Fetch discourse sources for instance filter
  const { data: sources } = useListSources();
  const discourseSources = useMemo(
    () => (sources ?? []).filter((s) => s.sourceType.startsWith("discourse")),
    [sources],
  );

  // Only fetch when section is expanded
  const enabled = open && hasDiscourse;
  const instance = instanceFilter === "all" ? undefined : instanceFilter;
  const { data, isLoading } = useDiscourseActivity(teamId, period, enabled, instance);

  if (!hasDiscourse) return null;

  const activityTrend = (data?.activityTrend ?? []).map((t) => ({
    date: t.date,
    topics: t.topics,
    posts: t.posts,
    likes: t.likes,
  }));

  const categories = data?.categoryDistribution ?? [];
  const allContributors = data?.topContributors ?? [];

  // Client-side search filtering
  const searchLower = debouncedSearch.toLowerCase();
  const filteredContributors = debouncedSearch
    ? allContributors.filter((c) => c.name.toLowerCase().includes(searchLower))
    : allContributors;

  // Client-side sorting
  const sortedContributors = sortContributors(filteredContributors, sorting);

  // Client-side pagination
  const totalCount = sortedContributors.length;
  const paginatedContributors = sortedContributors.slice(
    pageIndex * pageSize,
    (pageIndex + 1) * pageSize,
  );
  const hasNextPage = (pageIndex + 1) * pageSize < totalCount;

  const resetPage = (): void => setPageIndex(0);

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
            {/* Instance filter */}
            {discourseSources.length > 1 && (
              <div className="flex items-center gap-2">
                <span className="text-xs text-muted-foreground">Instance</span>
                <Select
                  value={instanceFilter}
                  onValueChange={(v) => {
                    if (v !== null) setInstanceFilter(v);
                    resetPage();
                  }}
                >
                  <SelectTrigger className="w-48">
                    <SelectValue>
                      {instanceFilter === "all" ? "All instances" : instanceFilter}
                    </SelectValue>
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="all">All instances</SelectItem>
                    {discourseSources.map((s) => (
                      <SelectItem key={s.sourceType} value={s.sourceType}>
                        {s.name || s.sourceType}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}

            {isLoading && enabled && (
              <div className="space-y-6">
                <div>
                  <Skeleton className="mb-3 h-4 w-32" />
                  <Skeleton className="h-[250px] w-full" />
                </div>
                <div>
                  <Skeleton className="mb-3 h-4 w-40" />
                  <Skeleton className="h-[160px] w-full" />
                </div>
                <div>
                  <Skeleton className="mb-3 h-4 w-36" />
                  <Skeleton className="h-10 w-full" />
                  <Skeleton className="mt-1 h-10 w-full" />
                  <Skeleton className="mt-1 h-10 w-full" />
                </div>
              </div>
            )}

            {!isLoading &&
              enabled &&
              activityTrend.length === 0 &&
              categories.length === 0 &&
              allContributors.length === 0 && (
                <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
                  <MessageCircle className="size-10 text-muted-foreground" />
                  <p className="mb-1 font-medium">No activity details</p>
                  <p className="text-sm text-muted-foreground">
                    Discourse activity exists but detailed breakdown is not yet available.
                  </p>
                </div>
              )}

            {!isLoading &&
              enabled &&
              (activityTrend.length > 0 || categories.length > 0 || allContributors.length > 0) && (
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
                  {allContributors.length > 0 && (
                    <div className="space-y-3">
                      <h4 className="text-sm font-medium">Top Contributors</h4>
                      <div className="flex flex-wrap items-center gap-4">
                        <div className="relative">
                          <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                          <Input
                            placeholder="Search..."
                            value={search}
                            onChange={(e) => handleSearchChange(e.target.value)}
                            className="h-8 w-48 pl-8 text-xs"
                          />
                        </div>
                      </div>
                      <div className="overflow-x-auto rounded-md border">
                        <DataTable
                          columns={contributorColumns}
                          data={paginatedContributors}
                          sorting={sorting}
                          onSortingChange={(updater) => {
                            setSorting(updater);
                            setPageIndex(0);
                          }}
                        />
                      </div>
                      <DataTablePagination
                        totalCount={totalCount}
                        pageSize={pageSize}
                        pageIndex={pageIndex}
                        hasNextPage={hasNextPage}
                        onPageSizeChange={(size) => {
                          setPageSize(size);
                          resetPage();
                        }}
                        onPreviousPage={() => setPageIndex((i) => Math.max(0, i - 1))}
                        onNextPage={() => setPageIndex((i) => i + 1)}
                      />
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
