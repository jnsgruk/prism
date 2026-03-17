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
import { ChartTooltip, cursorStyle } from "@/components/chart-tooltip";
import { instanceLabel } from "@/lib/format-metrics";
import { useDebouncedValue } from "@/lib/hooks/use-debounced-value";
import { ChevronDown, ChevronRight, ExternalLink, MessageCircle, Search } from "lucide-react";
import { useMemo, useState } from "react";
import type { ColumnDef, SortingState } from "@tanstack/react-table";
import type { Timestamp } from "@bufbuild/protobuf/wkt";
import {
  Area,
  AreaChart,
  CartesianGrid,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";

import type {
  Contribution,
  Period,
  TeamMetrics,
  TopContributor,
} from "@ps/api/gen/prism/v1/metrics_pb";
import type { ContributionFilters } from "@/lib/hooks/use-metrics";

import { useListSources } from "@/lib/hooks/use-config";
import { useListTeamContributions } from "@/lib/hooks/use-metrics";
import { useDiscourseActivity } from "@/views/teams/hooks/use-discourse-activity";

// ---------------------------------------------------------------------------
// Topics table columns
// ---------------------------------------------------------------------------

const discourseTypeLabel = (contributionType: string): string => {
  if (contributionType === "discourse_topic") return "Topic";
  if (contributionType === "discourse_like") return "Like";
  return "Post";
};

const formatShortDate = (ts?: Timestamp): string => {
  if (!ts) return "\u2014";
  const date = new Date(Number(ts.seconds) * 1000);
  return date.toLocaleDateString(undefined, { month: "short", day: "numeric" });
};

const topicTitleColumn: ColumnDef<Contribution, unknown> = {
  accessorKey: "title",
  header: "Title",
  cell: ({ row }) => {
    const c = row.original;
    return (
      <div className="flex min-w-0 items-center gap-1.5">
        <span className="block max-w-48 truncate" title={c.title}>
          {c.title || "\u2014"}
        </span>
        {c.url && (
          <a
            href={c.url}
            target="_blank"
            rel="noopener noreferrer"
            className="shrink-0 text-muted-foreground hover:text-foreground"
            onClick={(e) => e.stopPropagation()}
          >
            <ExternalLink className="size-3" />
          </a>
        )}
      </div>
    );
  },
  enableSorting: false,
};

const topicTypeColumn: ColumnDef<Contribution, unknown> = {
  id: "type",
  header: "Type",
  cell: ({ row }) => (
    <Badge variant="outline" className="text-[10px] uppercase">
      {discourseTypeLabel(row.original.contributionType)}
    </Badge>
  ),
  enableSorting: false,
};

const topicInstanceColumn: ColumnDef<Contribution, unknown> = {
  id: "instance",
  header: "Instance",
  cell: ({ row }) => (
    <span className="text-muted-foreground">{instanceLabel(row.original.platform)}</span>
  ),
  enableSorting: false,
};

const topicAuthorColumn: ColumnDef<Contribution, unknown> = {
  id: "person_name",
  accessorKey: "personName",
  header: "Author",
  cell: ({ row }) => (
    <span className="block max-w-28 truncate" title={row.original.personName}>
      {row.original.personName}
    </span>
  ),
  enableSorting: true,
};

const topicCreatedColumn: ColumnDef<Contribution, unknown> = {
  id: "created_at",
  accessorKey: "createdAt",
  header: "Created",
  cell: ({ row }) => (
    <span className="whitespace-nowrap text-muted-foreground">
      {formatShortDate(row.original.createdAt)}
    </span>
  ),
  enableSorting: true,
};

const buildTopicColumns = (showInstance: boolean): ColumnDef<Contribution, unknown>[] => [
  topicTitleColumn,
  topicTypeColumn,
  ...(showInstance ? [topicInstanceColumn] : []),
  topicAuthorColumn,
  topicCreatedColumn,
];

// ---------------------------------------------------------------------------
// Topics table sub-component (server-side pagination)
// ---------------------------------------------------------------------------

const DiscourseTopicsTable = ({
  teamId,
  period,
  platform,
}: {
  teamId: string;
  period: Period;
  platform?: string;
}): React.ReactElement => {
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebouncedValue(search);
  const [pageSize, setPageSize] = useState(10);
  const [pageIndex, setPageIndex] = useState(0);
  const [sorting, setSorting] = useState<SortingState>([]);

  const activeSortCol = sorting[0] as SortingState[number] | undefined;

  const filters: ContributionFilters = {
    contributionType: "discourse_%",
    search: debouncedSearch || undefined,
    sortField: activeSortCol?.id,
    sortDesc: activeSortCol?.desc,
    pageSize,
    pageIndex,
    platform,
  };

  const columns = useMemo(() => buildTopicColumns(!platform), [platform]);

  const { data, isLoading } = useListTeamContributions(teamId, period, filters);
  const topics = data?.contributions ?? [];
  const totalCount = data?.totalCount ?? 0;
  const hasNextPage = (pageIndex + 1) * pageSize < totalCount;

  return (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-4">
        <div className="relative">
          <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            placeholder="Search..."
            value={search}
            onChange={(e) => {
              setSearch(e.target.value);
              setPageIndex(0);
            }}
            className="h-8 w-48 pl-8 text-xs"
          />
        </div>
      </div>
      {isLoading ? (
        <p className="py-8 text-center text-sm text-muted-foreground">Loading topics...</p>
      ) : (
        <>
          <div className="overflow-x-auto rounded-md border">
            <DataTable
              columns={columns}
              data={topics}
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
              setPageIndex(0);
            }}
            onPreviousPage={() => setPageIndex((i) => Math.max(0, i - 1))}
            onNextPage={() => setPageIndex((i) => i + 1)}
          />
        </>
      )}
    </div>
  );
};

// ---------------------------------------------------------------------------
// Contributor table columns + sorting helper
// ---------------------------------------------------------------------------

const nameColumn: ColumnDef<TopContributor, unknown> = {
  id: "name",
  accessorKey: "name",
  header: "Name",
  cell: ({ row }) => (
    <span className="block max-w-40 truncate font-medium" title={row.original.name}>
      {row.original.name}
    </span>
  ),
  enableSorting: true,
};

const contributorTopicsColumn: ColumnDef<TopContributor, unknown> = {
  id: "topics",
  accessorKey: "topics",
  header: "Topics",
  cell: ({ row }) => <span className="tabular-nums text-right">{row.original.topics}</span>,
  enableSorting: true,
};

const contributorPostsColumn: ColumnDef<TopContributor, unknown> = {
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
  contributorTopicsColumn,
  contributorPostsColumn,
  likesColumn,
];

const sortContributors = (data: TopContributor[], sorting: SortingState): TopContributor[] => {
  const col = sorting[0];
  if (!col) return data;
  const { id, desc } = col;
  return data.toSorted((a, b) => {
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
};

// ---------------------------------------------------------------------------
// Main component
// ---------------------------------------------------------------------------

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

  // Contributor table state (client-side)
  const [contribSearch, setContribSearch] = useState("");
  const debouncedContribSearch = useDebouncedValue(contribSearch);
  const [contribPageSize, setContribPageSize] = useState(10);
  const [contribPageIndex, setContribPageIndex] = useState(0);
  const [contribSorting, setContribSorting] = useState<SortingState>([]);

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

  const activityTrend = useMemo(
    () =>
      (data?.activityTrend ?? []).map((t) => ({
        date: t.date,
        topics: t.topics,
        posts: t.posts,
        likes: t.likes,
      })),
    [data?.activityTrend],
  );

  const allContributors = data?.topContributors ?? [];

  // Client-side contributor filtering / sorting / pagination
  const searchLower = debouncedContribSearch.toLowerCase();
  const filteredContributors = debouncedContribSearch
    ? allContributors.filter((c) => c.name.toLowerCase().includes(searchLower))
    : allContributors;
  const sortedContributors = sortContributors(filteredContributors, contribSorting);
  const contribTotal = sortedContributors.length;
  const paginatedContributors = sortedContributors.slice(
    contribPageIndex * contribPageSize,
    (contribPageIndex + 1) * contribPageSize,
  );
  const contribHasNext = (contribPageIndex + 1) * contribPageSize < contribTotal;

  const resetContribPage = (): void => setContribPageIndex(0);

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
                    resetContribPage();
                  }}
                >
                  <SelectTrigger className="w-48">
                    <SelectValue>
                      {instanceFilter === "all"
                        ? "All instances"
                        : (discourseSources.find((s) => s.sourceType === instanceFilter)?.name ??
                          instanceFilter)}
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
              allContributors.length === 0 && (
                <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
                  <MessageCircle className="size-10 text-muted-foreground" />
                  <p className="mb-1 font-medium">No activity details</p>
                  <p className="text-sm text-muted-foreground">
                    Discourse activity exists but detailed breakdown is not yet available.
                  </p>
                </div>
              )}

            {!isLoading && enabled && (activityTrend.length > 0 || allContributors.length > 0) && (
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

                {/* Topics table (server-side paginated) */}
                <div>
                  <h4 className="mb-3 text-sm font-medium">Contributions</h4>
                  <DiscourseTopicsTable teamId={teamId} period={period} platform={instance} />
                </div>

                {/* Top contributors (client-side) */}
                {allContributors.length > 0 && (
                  <div className="space-y-3">
                    <h4 className="text-sm font-medium">Top Contributors</h4>
                    <div className="flex flex-wrap items-center gap-4">
                      <div className="relative">
                        <Search className="absolute left-2.5 top-1/2 size-3.5 -translate-y-1/2 text-muted-foreground" />
                        <Input
                          placeholder="Search..."
                          value={contribSearch}
                          onChange={(e) => {
                            setContribSearch(e.target.value);
                            resetContribPage();
                          }}
                          className="h-8 w-48 pl-8 text-xs"
                        />
                      </div>
                    </div>
                    <div className="overflow-x-auto rounded-md border">
                      <DataTable
                        columns={contributorColumns}
                        data={paginatedContributors}
                        sorting={contribSorting}
                        onSortingChange={(updater) => {
                          setContribSorting(updater);
                          setContribPageIndex(0);
                        }}
                      />
                    </div>
                    <DataTablePagination
                      totalCount={contribTotal}
                      pageSize={contribPageSize}
                      pageIndex={contribPageIndex}
                      hasNextPage={contribHasNext}
                      onPageSizeChange={(size) => {
                        setContribPageSize(size);
                        resetContribPage();
                      }}
                      onPreviousPage={() => setContribPageIndex((i) => Math.max(0, i - 1))}
                      onNextPage={() => setContribPageIndex((i) => i + 1)}
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
