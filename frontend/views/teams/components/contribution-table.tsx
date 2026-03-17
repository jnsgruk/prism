import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { ExternalLink, Search } from "lucide-react";
import { useRef, useMemo, useState } from "react";
import type { ColumnDef, SortingState } from "@tanstack/react-table";
import type { Timestamp } from "@bufbuild/protobuf/wkt";

import type { Period } from "@ps/api/gen/prism/v1/metrics_pb";
import type {
  Contribution,
  ContributionFilters,
  PersonContributionFilters,
} from "@/lib/hooks/use-metrics";
import { useListTeamContributions, useListPersonContributions } from "@/lib/hooks/use-metrics";

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
      return "default";
    case "open":
      return "outline";
    case "closed":
      return "destructive";
    case "approved":
      return "default";
    case "changes_requested":
      return "destructive";
    default:
      return "secondary";
  }
};

/** Extract PR/review number from platform_id like "owner/repo/pull/123" or "owner/repo/review/456". */
const extractNumber = (platformId: string): string | null => {
  const last = platformId.split("/").pop();
  return last && /^\d+$/.test(last) ? last : null;
};

const prTitleColumn: ColumnDef<Contribution, unknown> = {
  accessorKey: "title",
  header: "PR",
  cell: ({ row }) => {
    const c = row.original;
    const num = extractNumber(c.platformId);
    const label = num ? `#${num}` : c.title || "\u2014";
    return (
      <div className="flex min-w-0 items-center gap-1.5">
        <span className="whitespace-nowrap" title={c.title}>
          {label}
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

const reviewTitleColumn: ColumnDef<Contribution, unknown> = {
  accessorKey: "title",
  header: "Title",
  cell: ({ row }) => (
    <div className="flex min-w-0 items-center gap-1.5">
      <span className="truncate" title={row.original.title}>
        {row.original.title || "\u2014"}
      </span>
      {row.original.url && (
        <a
          href={row.original.url}
          target="_blank"
          rel="noopener noreferrer"
          className="shrink-0 text-muted-foreground hover:text-foreground"
          onClick={(e) => e.stopPropagation()}
        >
          <ExternalLink className="size-3" />
        </a>
      )}
    </div>
  ),
  enableSorting: false,
};

const authorColumn: ColumnDef<Contribution, unknown> = {
  id: "person_name",
  accessorKey: "personName",
  header: "Author",
  cell: ({ row }) => (
    <span className="block max-w-40 truncate" title={row.original.personName}>
      {row.original.personName}
    </span>
  ),
  enableSorting: true,
};

const repoColumn: ColumnDef<Contribution, unknown> = {
  id: "repo",
  accessorKey: "repo",
  header: "Repo",
  cell: ({ row }) => (
    <span className="block max-w-30 truncate text-muted-foreground" title={row.original.repo}>
      {row.original.repo || "\u2014"}
    </span>
  ),
  enableSorting: true,
};

const prStateColumn: ColumnDef<Contribution, unknown> = {
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

const reviewStateColumn: ColumnDef<Contribution, unknown> = {
  id: "state",
  accessorKey: "state",
  header: "State",
  cell: ({ row }) =>
    row.original.state ? (
      <Badge variant={stateBadgeVariant(row.original.state)} className="text-[10px]">
        {row.original.state}
      </Badge>
    ) : (
      "\u2014"
    ),
  enableSorting: true,
};

const createdAtColumn: ColumnDef<Contribution, unknown> = {
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

const prStatsColumn: ColumnDef<Contribution, unknown> = {
  id: "stats",
  header: "Stats",
  cell: ({ row }) => {
    const c = row.original;
    return c.additions > 0 || c.deletions > 0 ? (
      <span className="whitespace-nowrap">
        <span className="text-green-600">+{c.additions}</span>{" "}
        <span className="text-red-600">-{c.deletions}</span>
      </span>
    ) : (
      "\u2014"
    );
  },
  enableSorting: false,
};

type PrStateFilter = "all" | "merged" | "open" | "closed";
type ReviewStateFilter = "all" | "APPROVED" | "COMMENTED" | "CHANGES_REQUESTED" | "DISMISSED";
type StateFilter = PrStateFilter | ReviewStateFilter;

const prStates: PrStateFilter[] = ["all", "merged", "open", "closed"];
const reviewStates: ReviewStateFilter[] = [
  "all",
  "APPROVED",
  "COMMENTED",
  "CHANGES_REQUESTED",
  "DISMISSED",
];

const stateLabel = (s: string): string => {
  const labels: Record<string, string> = {
    all: "All",
    merged: "Merged",
    open: "Open",
    closed: "Closed",
    APPROVED: "Approved",
    COMMENTED: "Commented",
    CHANGES_REQUESTED: "Changes Requested",
    DISMISSED: "Dismissed",
  };
  return labels[s] ?? s;
};

export const ContributionTable = ({
  teamId,
  personId,
  period,
  defaultContributionType,
  defaultState,
}: {
  teamId?: string;
  personId?: string;
  period?: Period;
  defaultContributionType?: string;
  defaultState?: string;
}): React.ReactElement => {
  const isReview = defaultContributionType === "pr_review";
  const [stateFilter, setStateFilter] = useState<StateFilter>(
    (defaultState as StateFilter) ?? "all",
  );

  const activeStates: StateFilter[] = isReview ? reviewStates : prStates;
  const columns = useMemo(
    (): ColumnDef<Contribution, unknown>[] =>
      isReview
        ? [reviewTitleColumn, authorColumn, repoColumn, reviewStateColumn, createdAtColumn]
        : [prTitleColumn, authorColumn, repoColumn, prStateColumn, createdAtColumn, prStatsColumn],
    [isReview],
  );
  const [search, setSearch] = useState("");
  const [debouncedSearch, setDebouncedSearch] = useState("");
  const [pageSize, setPageSize] = useState(10);
  const [pageIndex, setPageIndex] = useState(0);
  const [sorting, setSorting] = useState<SortingState>([]);

  // Debounce search input
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
  const sortField = activeSortCol?.id;
  const sortDesc = activeSortCol?.desc;

  const teamFilters: ContributionFilters = {
    contributionType: defaultContributionType,
    state: stateFilter === "all" ? undefined : stateFilter,
    search: debouncedSearch || undefined,
    sortField,
    sortDesc,
    pageSize,
    pageIndex,
  };

  const personFilters: PersonContributionFilters = {
    contributionType: defaultContributionType,
    state: stateFilter === "all" ? undefined : stateFilter,
    search: debouncedSearch || undefined,
    sortField,
    sortDesc,
    pageSize,
    pageIndex,
  };

  const teamQuery = useListTeamContributions(teamId ?? "", period!, teamFilters);
  const personQuery = useListPersonContributions(personId ?? "", personFilters);

  const isPersonMode = !!personId;
  const data = isPersonMode ? personQuery.data : teamQuery.data;
  const isLoading = isPersonMode ? personQuery.isLoading : teamQuery.isLoading;

  const contributions = data?.contributions ?? [];
  const totalCount = data?.totalCount ?? 0;
  const hasNextPage = (pageIndex + 1) * pageSize < totalCount;

  const resetPage = (): void => setPageIndex(0);

  return (
    <div className="space-y-3">
      {/* Filters */}
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
          <span className="text-xs text-muted-foreground">State</span>
          <div className="flex gap-0.5">
            {activeStates.map((s) => (
              <button
                key={s}
                onClick={() => {
                  setStateFilter(s);
                  resetPage();
                }}
                className={`rounded-md px-2.5 py-1 text-xs font-medium transition-colors ${
                  stateFilter === s
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground hover:bg-muted/80"
                }`}
              >
                {stateLabel(s)}
              </button>
            ))}
          </div>
        </div>
      </div>

      {/* Table */}
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
              resetPage();
            }}
            onPreviousPage={() => setPageIndex((i) => Math.max(0, i - 1))}
            onNextPage={() => setPageIndex((i) => i + 1)}
          />
        </>
      )}
    </div>
  );
};
