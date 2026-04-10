import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import { useDebouncedValue } from "@/lib/hooks/use-debounced-value";
import type { Contribution, ContributionFilters, PersonContributionFilters } from "@/lib/hooks/use-metrics";
import { useListTeamContributions, useListPersonContributions } from "@/lib/hooks/use-metrics";
import { contributionStateLabel, platformLabel } from "@/lib/proto-display";
import { create } from "@bufbuild/protobuf";
import type { Timestamp } from "@bufbuild/protobuf/wkt";
import type { ColumnDef, SortingState } from "@tanstack/react-table";
import { ExternalLink, Search } from "lucide-react";
import { useCallback, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";

import { ContributionState, ContributionType, Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { Period } from "@ps/api/gen/canonical/prism/v1/metrics_pb";
import { PeriodSchema } from "@ps/api/gen/canonical/prism/v1/metrics_pb";

const formatTimestamp = (ts?: Timestamp): string => {
  if (!ts) return "\u2014";
  const date = new Date(Number(ts.seconds) * 1000);
  return (
    date.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
    " " +
    date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit", hour12: false })
  );
};

const stateBadgeVariant = (state: ContributionState): "default" | "secondary" | "destructive" | "outline" => {
  switch (state) {
    case ContributionState.MERGED:
    case ContributionState.APPROVED:
      return "default";
    case ContributionState.OPEN:
      return "outline";
    case ContributionState.CLOSED:
    case ContributionState.CHANGES_REQUESTED:
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

const prTitleColumn: ColumnDef<Contribution> = {
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

const reviewTitleColumn: ColumnDef<Contribution> = {
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

const authorColumn: ColumnDef<Contribution> = {
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

const repoColumn: ColumnDef<Contribution> = {
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

const prStateColumn: ColumnDef<Contribution> = {
  id: "state",
  accessorKey: "state",
  header: "State",
  cell: ({ row }) =>
    row.original.state ? (
      <Badge variant={stateBadgeVariant(row.original.state)} className="text-[10px] uppercase">
        {contributionStateLabel(row.original.state)}
      </Badge>
    ) : (
      "\u2014"
    ),
  enableSorting: true,
};

const reviewStateColumn: ColumnDef<Contribution> = {
  id: "state",
  accessorKey: "state",
  header: "State",
  cell: ({ row }) =>
    row.original.state ? (
      <Badge variant={stateBadgeVariant(row.original.state)} className="text-[10px]">
        {contributionStateLabel(row.original.state)}
      </Badge>
    ) : (
      "\u2014"
    ),
  enableSorting: true,
};

const createdAtColumn: ColumnDef<Contribution> = {
  id: "created_at",
  accessorKey: "createdAt",
  header: "Created",
  cell: ({ row }) => (
    <span className="whitespace-nowrap text-muted-foreground">{formatTimestamp(row.original.createdAt)}</span>
  ),
  enableSorting: true,
};

const prStatsColumn: ColumnDef<Contribution> = {
  id: "stats",
  header: "Stats",
  cell: ({ row }) => {
    const c = row.original;
    return c.additions > 0 || c.deletions > 0 ? (
      <span className="whitespace-nowrap">
        <span className="text-green-600">+{c.additions}</span> <span className="text-red-600">-{c.deletions}</span>
      </span>
    ) : (
      "\u2014"
    );
  },
  enableSorting: false,
};

// ---------------------------------------------------------------------------
// Discourse columns
// ---------------------------------------------------------------------------

const discourseTitleColumn: ColumnDef<Contribution> = {
  accessorKey: "title",
  header: "Title",
  cell: ({ row }) => {
    const c = row.original;
    return (
      <div className="flex min-w-0 items-center gap-1.5">
        <span className="block max-w-60 truncate" title={c.title}>
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

const discourseTypeLabel = (ct: ContributionType): string => {
  if (ct === ContributionType.DISCOURSE_TOPIC) return "Topic";
  if (ct === ContributionType.DISCOURSE_LIKE) return "Like";
  return "Post";
};

const discourseTypeColumn: ColumnDef<Contribution> = {
  id: "type",
  header: "Type",
  cell: ({ row }) => (
    <Badge variant="outline" className="text-[10px] uppercase">
      {discourseTypeLabel(row.original.contributionType)}
    </Badge>
  ),
  enableSorting: false,
};

const discourseInstanceColumn: ColumnDef<Contribution> = {
  id: "instance",
  header: "Instance",
  cell: ({ row }) => <span className="text-muted-foreground">{platformLabel(row.original.platform)}</span>,
  enableSorting: false,
};

// ---------------------------------------------------------------------------
// State filters
// ---------------------------------------------------------------------------

type StateFilterValue = "all" | ContributionState;

const prStates: StateFilterValue[] = [
  "all",
  ContributionState.MERGED,
  ContributionState.OPEN,
  ContributionState.CLOSED,
];
const reviewStates: StateFilterValue[] = [
  "all",
  ContributionState.APPROVED,
  ContributionState.COMMENTED,
  ContributionState.CHANGES_REQUESTED,
  ContributionState.DISMISSED,
];

const parseStateFilter = (value?: ContributionState): StateFilterValue => value ?? "all";

const stateLabel = (s: StateFilterValue): string => {
  if (s === "all") return "All";
  return contributionStateLabel(s);
};

export const ContributionTable = ({
  teamId,
  personId,
  period,
  defaultContributionType,
  defaultState,
  defaultPlatform,
}: {
  teamId?: string;
  personId?: string;
  period?: Period;
  defaultContributionType?: ContributionType;
  defaultState?: ContributionState;
  defaultPlatform?: Platform;
}): React.ReactElement => {
  const isReview = defaultContributionType === ContributionType.PR_REVIEW;
  const isDiscourse = defaultPlatform === Platform.DISCOURSE;
  const [stateFilter, setStateFilter] = useState<StateFilterValue>(parseStateFilter(defaultState));

  const activeStates: StateFilterValue[] = (() => {
    if (isDiscourse) return [];
    if (isReview) return reviewStates;
    return prStates;
  })();

  const navigate = useNavigate();
  const handleRowClick = useCallback(
    (row: Contribution) => {
      navigate(`/contributions/${row.id}`);
    },
    [navigate],
  );

  const isPersonMode = !!personId;
  const columns = useMemo((): ColumnDef<Contribution>[] => {
    if (isDiscourse) {
      return isPersonMode
        ? [discourseTitleColumn, discourseTypeColumn, discourseInstanceColumn, createdAtColumn]
        : [discourseTitleColumn, discourseTypeColumn, discourseInstanceColumn, authorColumn, createdAtColumn];
    }
    if (isReview) {
      return isPersonMode
        ? [reviewTitleColumn, repoColumn, reviewStateColumn, createdAtColumn]
        : [reviewTitleColumn, authorColumn, repoColumn, reviewStateColumn, createdAtColumn];
    }
    return isPersonMode
      ? [prTitleColumn, repoColumn, prStateColumn, createdAtColumn, prStatsColumn]
      : [prTitleColumn, authorColumn, repoColumn, prStateColumn, createdAtColumn, prStatsColumn];
  }, [isReview, isDiscourse, isPersonMode]);
  const [search, setSearch] = useState("");
  const debouncedSearch = useDebouncedValue(search);
  const [pageSize, setPageSize] = useState(10);
  const [pageIndex, setPageIndex] = useState(0);
  const [sorting, setSorting] = useState<SortingState>([]);

  const activeSortCol = sorting[0] as SortingState[number] | undefined;
  const sortField = activeSortCol?.id;
  const sortDesc = activeSortCol?.desc;

  const activeState = stateFilter === "all" ? undefined : stateFilter;

  const teamFilters: ContributionFilters = {
    contributionType: defaultContributionType,
    state: activeState,
    search: debouncedSearch || undefined,
    sortField,
    sortDesc,
    pageSize,
    pageIndex,
    platform: defaultPlatform,
  };

  const personFilters: PersonContributionFilters = {
    contributionType: isDiscourse ? undefined : defaultContributionType,
    platform: defaultPlatform,
    state: activeState,
    search: debouncedSearch || undefined,
    sortField,
    sortDesc,
    pageSize,
    pageIndex,
    since: period?.start || undefined,
    until: period?.end || undefined,
  };

  // Both hooks must always be called (React rules of hooks).
  // Use a dummy period for the team hook when in person mode to avoid
  // accessing undefined period.type in the query key.
  const dummyPeriod = create(PeriodSchema, { start: "", end: "" });
  const teamQuery = useListTeamContributions(isPersonMode ? "" : (teamId ?? ""), period ?? dummyPeriod, teamFilters);
  const personQuery = useListPersonContributions(isPersonMode ? (personId ?? "") : "", personFilters);

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
            onChange={(e) => {
              setSearch(e.target.value);
              setPageIndex(0);
            }}
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
              onRowClick={handleRowClick}
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
