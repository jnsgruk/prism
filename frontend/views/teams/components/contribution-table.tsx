import { Badge } from "@/components/ui/badge";
import { DataTable } from "@/components/data-table/data-table";
import { DataTablePagination } from "@/components/data-table/data-table-pagination";
import { ExternalLink } from "lucide-react";
import { useState } from "react";
import type { ColumnDef, SortingState } from "@tanstack/react-table";
import type { Timestamp } from "@bufbuild/protobuf/wkt";

import type { Period } from "@ps/api/gen/prism/v1/metrics_pb";
import type { Contribution, ContributionFilters } from "@/lib/hooks/use-metrics";
import { useListTeamContributions } from "@/lib/hooks/use-metrics";

const formatTimestamp = (ts?: Timestamp): string => {
  if (!ts) return "\u2014";
  const date = new Date(Number(ts.seconds) * 1000);
  return (
    date.toLocaleDateString(undefined, { month: "short", day: "numeric" }) +
    " " +
    date.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" })
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

const columns: ColumnDef<Contribution, unknown>[] = [
  {
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
  },
  {
    accessorKey: "personName",
    header: "Author",
    enableSorting: false,
  },
  {
    accessorKey: "repo",
    header: "Repo",
    cell: ({ row }) => (
      <span className="text-muted-foreground">{row.original.repo || "\u2014"}</span>
    ),
    enableSorting: false,
  },
  {
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
    enableSorting: false,
  },
  {
    accessorKey: "createdAt",
    header: "Created",
    cell: ({ row }) => (
      <span className="whitespace-nowrap text-muted-foreground">
        {formatTimestamp(row.original.createdAt)}
      </span>
    ),
    enableSorting: true,
  },
  {
    id: "stats",
    header: "Stats",
    cell: ({ row }) => {
      const c = row.original;
      if (c.contributionType === "pr_review") {
        return c.reviewHours > 0 ? (
          <span className="whitespace-nowrap">{c.reviewHours.toFixed(1)}h turnaround</span>
        ) : (
          "\u2014"
        );
      }
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
  },
];

type TypeFilter = "all" | "pull_request" | "pr_review";
type StateFilter = "all" | "merged" | "open" | "closed";

export const ContributionTable = ({
  teamId,
  period,
  defaultContributionType,
  defaultState,
}: {
  teamId: string;
  period: Period;
  defaultContributionType?: string;
  defaultState?: string;
}): React.ReactElement => {
  const [typeFilter, setTypeFilter] = useState<TypeFilter>(
    (defaultContributionType as TypeFilter) ?? "all",
  );
  const [stateFilter, setStateFilter] = useState<StateFilter>(
    (defaultState as StateFilter) ?? "all",
  );
  const [pageSize, setPageSize] = useState(25);
  const [pageIndex, setPageIndex] = useState(0);
  const [sorting, setSorting] = useState<SortingState>([]);

  const filters: ContributionFilters = {
    contributionType: typeFilter === "all" ? undefined : typeFilter,
    state: stateFilter === "all" ? undefined : stateFilter,
    pageSize,
    pageIndex,
  };

  const { data, isLoading } = useListTeamContributions(teamId, period, filters);

  const contributions = data?.contributions ?? [];
  const totalCount = data?.totalCount ?? 0;
  const hasNextPage = (pageIndex + 1) * pageSize < totalCount;

  const resetPage = (): void => setPageIndex(0);

  return (
    <div className="space-y-3">
      {/* Filters */}
      <div className="flex flex-wrap gap-4">
        <div className="flex items-center gap-1.5">
          <span className="text-xs text-muted-foreground">Type</span>
          <div className="flex gap-0.5">
            {(["all", "pull_request", "pr_review"] as const).map((t) => (
              <button
                key={t}
                onClick={() => {
                  setTypeFilter(t);
                  resetPage();
                }}
                className={`rounded-md px-2.5 py-1 text-xs font-medium transition-colors ${
                  typeFilter === t
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground hover:bg-muted/80"
                }`}
              >
                {{ all: "All", pull_request: "PRs", pr_review: "Reviews" }[t]}
              </button>
            ))}
          </div>
        </div>
        <div className="flex items-center gap-1.5">
          <span className="text-xs text-muted-foreground">State</span>
          <div className="flex gap-0.5">
            {(["all", "merged", "open", "closed"] as const).map((s) => (
              <button
                key={s}
                onClick={() => {
                  setStateFilter(s);
                  resetPage();
                }}
                className={`rounded-md px-2.5 py-1 text-xs font-medium capitalize transition-colors ${
                  stateFilter === s
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground hover:bg-muted/80"
                }`}
              >
                {s}
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
              onSortingChange={setSorting}
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
