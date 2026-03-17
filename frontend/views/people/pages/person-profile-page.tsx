import { useParams, useNavigate, Link } from "react-router-dom";
import { useState, useRef, useMemo } from "react";
import {
  ArrowLeft,
  Activity,
  BarChart3,
  ExternalLink,
  Loader2,
  Search,
  TrendingUp,
  Users,
} from "lucide-react";
import type { ColumnDef, SortingState } from "@tanstack/react-table";
import type { Timestamp } from "@bufbuild/protobuf/wkt";
import { Bar, BarChart, CartesianGrid, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import type { TooltipContentProps } from "recharts/types/component/Tooltip";

import { PageHeader } from "@/components/page-header";
import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Input } from "@/components/ui/input";
import { Tooltip as UITooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  Breadcrumb,
  BreadcrumbItem,
  BreadcrumbLink,
  BreadcrumbList,
  BreadcrumbPage,
  BreadcrumbSeparator,
} from "@/components/ui/breadcrumb";
import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import {
  PeriodSelector,
  buildPeriod,
  defaultPeriodKey,
} from "@/views/teams/components/period-selector";
import type { Contribution, GetIndividualProfileResponse } from "@/lib/hooks/use-metrics";
import { useGetIndividualProfile, useListPersonContributions } from "@/lib/hooks/use-metrics";
import type { PersonContributionFilters } from "@/lib/hooks/use-metrics";

// ---------------------------------------------------------------------------
// Helpers
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

const stateBadgeVariant = (state: string): "default" | "secondary" | "destructive" | "outline" => {
  switch (state.toLowerCase()) {
    case "merged":
    case "approved":
      return "default";
    case "open":
      return "outline";
    case "closed":
    case "changes_requested":
      return "destructive";
    default:
      return "secondary";
  }
};

const fmtFloat = (v: number): string => (v > 0 ? v.toFixed(1) : "\u2014");
const fmtPercent = (v: number): string => `${Math.round(v * 100)}%`;

// ---------------------------------------------------------------------------
// Chart tooltip
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Metric cards
// ---------------------------------------------------------------------------

const MetricCard = ({
  label,
  value,
  icon: Icon,
  description,
}: {
  label: string;
  value: string;
  icon: React.ComponentType<{ className?: string }>;
  description?: string;
}): React.ReactElement => (
  <Card>
    <CardContent className="flex items-center gap-3 p-4">
      <div className="rounded-md bg-muted p-2">
        <Icon className="size-4 text-muted-foreground" />
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-2xl font-semibold leading-none">{value}</p>
        <div className="mt-1 flex items-center gap-1">
          <p className="text-xs text-muted-foreground">{label}</p>
          {description && (
            <UITooltip>
              <TooltipTrigger render={<button type="button" className="inline-flex shrink-0" />}>
                <Activity className="size-3 text-muted-foreground/50" />
              </TooltipTrigger>
              <TooltipContent side="bottom" className="max-w-64">
                {description}
              </TooltipContent>
            </UITooltip>
          )}
        </div>
      </div>
    </CardContent>
  </Card>
);

const ProfileMetricCards = ({
  profile,
}: {
  profile: GetIndividualProfileResponse;
}): React.ReactElement => {
  const totalContributions = profile.activityByPlatform.reduce(
    (sum, a) => sum + a.contributionCount,
    0,
  );
  const platformCount = profile.activityByPlatform.length;
  const peerPercentile = profile.peerContext?.metrics["throughput"];

  return (
    <div className="grid grid-cols-2 gap-4 lg:grid-cols-4">
      <MetricCard
        label="Contributions"
        value={String(totalContributions)}
        icon={Activity}
        description="Total contributions across all platforms in this period"
      />
      <MetricCard label="Platforms active" value={String(platformCount)} icon={BarChart3} />
      {peerPercentile ? (
        <MetricCard
          label="Peer percentile"
          value={fmtPercent(peerPercentile.percentile)}
          icon={TrendingUp}
          description={`Throughput percentile among ${profile.peerContext?.peerCount ?? 0} ${profile.peerContext?.level ?? ""} peers`}
        />
      ) : (
        <MetricCard label="Peer percentile" value="\u2014" icon={TrendingUp} />
      )}
      <MetricCard label="Identities" value={String(profile.identities.length)} icon={Users} />
    </div>
  );
};

// ---------------------------------------------------------------------------
// Activity by platform chart
// ---------------------------------------------------------------------------

const ActivityChart = ({
  profile,
}: {
  profile: GetIndividualProfileResponse;
}): React.ReactElement | null => {
  if (profile.activityByPlatform.length === 0) return null;

  const data = profile.activityByPlatform.map((a) => ({
    platform: a.platform,
    count: a.contributionCount,
  }));

  return (
    <Card>
      <CardHeader>
        <CardTitle>Activity by platform</CardTitle>
        <CardDescription>Contribution counts per platform in this period</CardDescription>
      </CardHeader>
      <CardContent>
        <ResponsiveContainer width="100%" height={250}>
          <BarChart data={data} margin={{ top: 5, right: 30, left: 0, bottom: 5 }}>
            <CartesianGrid strokeDasharray="3 3" className="stroke-border" />
            <XAxis dataKey="platform" tick={{ fontSize: 12 }} className="fill-muted-foreground" />
            <YAxis className="fill-muted-foreground" allowDecimals={false} />
            <Tooltip content={ChartTooltip} cursor={{ fill: "hsl(var(--muted))", opacity: 0.5 }} />
            <Bar
              dataKey="count"
              name="Contributions"
              fill="hsl(var(--primary))"
              radius={[4, 4, 0, 0]}
            />
          </BarChart>
        </ResponsiveContainer>
        {/* Per-platform key metrics */}
        <div className="mt-4 grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-3">
          {profile.activityByPlatform.map((a) => (
            <div key={a.platform} className="rounded-md border px-3 py-2">
              <p className="text-sm font-medium">{a.platform}</p>
              <p className="text-xs text-muted-foreground">
                {a.contributionCount} contribution{a.contributionCount !== 1 ? "s" : ""}
                {a.metrics["avg_review_hours"] != null &&
                  ` \u00b7 avg review ${fmtFloat(a.metrics["avg_review_hours"])}h`}
                {a.metrics["avg_cycle_time_hours"] != null &&
                  ` \u00b7 avg cycle ${fmtFloat(a.metrics["avg_cycle_time_hours"])}h`}
              </p>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
};

// ---------------------------------------------------------------------------
// Peer context panel
// ---------------------------------------------------------------------------

const PeerContextPanel = ({
  profile,
}: {
  profile: GetIndividualProfileResponse;
}): React.ReactElement | null => {
  const peer = profile.peerContext;
  if (!peer || peer.peerCount === 0) return null;

  return (
    <Card>
      <CardHeader>
        <CardTitle>Peer context</CardTitle>
        <CardDescription>
          Compared to {peer.peerCount} other {peer.level} peers in this period
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="space-y-2">
          {Object.entries(peer.metrics).map(([name, p]) => (
            <div
              key={name}
              className="flex items-center justify-between rounded-md border px-3 py-2"
            >
              <span className="text-sm capitalize">{name.replace(/_/g, " ")}</span>
              <div className="flex items-center gap-2">
                <span className="tabular-nums text-sm font-medium">{fmtFloat(p.value)}</span>
                <Badge variant="secondary" className="text-[10px]">
                  {fmtPercent(p.percentile)} percentile
                </Badge>
              </div>
            </div>
          ))}
        </div>
      </CardContent>
    </Card>
  );
};

// ---------------------------------------------------------------------------
// Contributions table (person-scoped)
// ---------------------------------------------------------------------------

const titleColumn: ColumnDef<Contribution, unknown> = {
  accessorKey: "title",
  header: "Title",
  cell: ({ row }) => {
    const c = row.original;
    return (
      <div className="flex min-w-0 items-center gap-1.5">
        <span className="truncate" title={c.title}>
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

const platformColumn: ColumnDef<Contribution, unknown> = {
  id: "platform",
  accessorKey: "platform",
  header: "Platform",
  cell: ({ row }) => (
    <Badge variant="secondary" className="text-[10px]">
      {row.original.platform}
    </Badge>
  ),
  enableSorting: true,
};

const typeColumn: ColumnDef<Contribution, unknown> = {
  accessorKey: "contributionType",
  header: "Type",
  cell: ({ row }) => (
    <span className="text-xs text-muted-foreground">{row.original.contributionType}</span>
  ),
  enableSorting: false,
};

const stateColumn: ColumnDef<Contribution, unknown> = {
  id: "state",
  accessorKey: "state",
  header: "State",
  cell: ({ row }) =>
    row.original.state ? (
      <Badge variant={stateBadgeVariant(row.original.state)} className="text-[10px] uppercase">
        {row.original.state}
      </Badge>
    ) : (
      "\u2014"
    ),
  enableSorting: true,
};

const createdColumn: ColumnDef<Contribution, unknown> = {
  id: "created_at",
  accessorKey: "createdAt",
  header: "Created",
  cell: ({ row }) => (
    <span className="whitespace-nowrap text-muted-foreground">
      {formatTimestamp(row.original.createdAt)}
    </span>
  ),
  enableSorting: true,
};

const columns: ColumnDef<Contribution, unknown>[] = [
  titleColumn,
  platformColumn,
  typeColumn,
  stateColumn,
  createdColumn,
];

const PersonContributions = ({ personId }: { personId: string }): React.ReactElement => {
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [platformFilter, setPlatformFilter] = useState<string>("");
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

  const activeSortCol = sorting[0] as SortingState[number] | undefined;

  const filters: PersonContributionFilters = {
    platform: platformFilter || undefined,
    sortField: activeSortCol?.id,
    sortDesc: activeSortCol?.desc,
    pageSize,
    pageIndex,
  };

  const { data, isLoading } = useListPersonContributions(personId, filters);

  // Client-side title search filter
  const contributions = useMemo(() => {
    const items = data?.contributions ?? [];
    if (!debouncedSearch) return items;
    const q = debouncedSearch.toLowerCase();
    return items.filter(
      (c) =>
        c.title.toLowerCase().includes(q) ||
        c.platform.toLowerCase().includes(q) ||
        c.contributionType.toLowerCase().includes(q),
    );
  }, [data?.contributions, debouncedSearch]);

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
            onChange={(e) => handleSearchChange(e.target.value)}
            className="h-8 w-48 pl-8 text-xs"
          />
        </div>
        <div className="flex items-center gap-1.5">
          <span className="text-xs text-muted-foreground">Platform</span>
          <div className="flex gap-0.5">
            {["", "github", "jira"].map((p) => (
              <button
                key={p}
                onClick={() => {
                  setPlatformFilter(p);
                  setPageIndex(0);
                }}
                className={`rounded-md px-2.5 py-1 text-xs font-medium transition-colors ${
                  platformFilter === p
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground hover:bg-muted/80"
                }`}
              >
                {p || "All"}
              </button>
            ))}
          </div>
        </div>
      </div>

      {isLoading ? (
        <p className="py-8 text-center text-sm text-muted-foreground">Loading contributions...</p>
      ) : (
        <>
          <div className="overflow-x-auto rounded-md border">
            <DataTable
              columns={columns}
              data={contributions}
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
// Page
// ---------------------------------------------------------------------------

const PersonProfilePage = (): React.ReactElement => {
  const { personId } = useParams<{ personId: string }>();
  const navigate = useNavigate();
  const [periodKey, setPeriodKey] = useState(defaultPeriodKey);
  const period = buildPeriod(periodKey);

  const { data: profile, isLoading, error } = useGetIndividualProfile(personId ?? "", period);

  if (!personId) {
    return (
      <div className="flex flex-1 items-center justify-center">
        <p className="text-muted-foreground">No person selected.</p>
      </div>
    );
  }

  const description = [profile?.teamName, profile?.level].filter(Boolean).join(" \u00b7 ");

  return (
    <>
      <PageHeader
        title={profile?.name ?? "Person"}
        description={description || undefined}
        actions={
          <button
            type="button"
            onClick={() => navigate(-1)}
            className="flex items-center gap-1 text-sm text-muted-foreground hover:text-foreground"
          >
            <ArrowLeft className="size-4" />
            Back
          </button>
        }
      />
      <div className="min-w-0 flex-1 space-y-6 overflow-y-auto p-6">
        {/* Period selector + breadcrumb */}
        <div className="space-y-3">
          <PeriodSelector value={periodKey} onChange={setPeriodKey} />
          {profile?.teamName && (
            <Breadcrumb>
              <BreadcrumbList>
                <BreadcrumbItem>
                  <BreadcrumbLink render={<Link to="/teams" />}>Teams</BreadcrumbLink>
                </BreadcrumbItem>
                <BreadcrumbSeparator />
                <BreadcrumbItem>
                  <BreadcrumbLink render={<Link to="/teams" />}>{profile.teamName}</BreadcrumbLink>
                </BreadcrumbItem>
                <BreadcrumbSeparator />
                <BreadcrumbItem>
                  <BreadcrumbPage>{profile.name}</BreadcrumbPage>
                </BreadcrumbItem>
              </BreadcrumbList>
            </Breadcrumb>
          )}
        </div>

        {/* Loading */}
        {isLoading && (
          <div className="flex justify-center py-12">
            <Loader2 className="size-6 animate-spin text-muted-foreground" />
          </div>
        )}

        {/* Error */}
        {error && (
          <Alert variant="destructive">
            <AlertDescription>
              {error instanceof Error ? error.message : "Failed to load profile"}
            </AlertDescription>
          </Alert>
        )}

        {/* Profile content */}
        {profile && !isLoading && (
          <>
            {/* Platform identities */}
            {profile.identities.length > 0 && (
              <div className="flex flex-wrap gap-1.5">
                {profile.identities.map((id) => (
                  <Badge key={`${id.platform}-${id.username}`} variant="outline">
                    {id.platform}: {id.username}
                  </Badge>
                ))}
              </div>
            )}

            {/* Metric cards */}
            <ProfileMetricCards profile={profile} />

            {/* Activity chart */}
            <ActivityChart profile={profile} />

            {/* Peer context */}
            <PeerContextPanel profile={profile} />

            {/* Contributions */}
            <Card>
              <CardHeader>
                <CardTitle>Contributions</CardTitle>
                <CardDescription>All contributions across platforms</CardDescription>
              </CardHeader>
              <CardContent>
                <PersonContributions personId={personId} />
              </CardContent>
            </Card>
          </>
        )}
      </div>
    </>
  );
};

export default PersonProfilePage;
