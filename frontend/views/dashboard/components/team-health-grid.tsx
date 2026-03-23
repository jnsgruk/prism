import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { SentimentBar } from "@/components/sentiment-bar";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { ArrowUpDown, UsersRound } from "lucide-react";
import { useMemo, useState } from "react";
import { Link } from "react-router-dom";

import type { TeamReviewComparison } from "@ps/api/gen/canonical/prism/v1/insights_pb";

type SortField = "name" | "reviewCount" | "avgDepth" | "rubberStampPct";
type SortDir = "asc" | "desc";

const depthColor = (depth: number): string => {
  if (depth >= 2.8) return "text-emerald-600";
  if (depth >= 2.2) return "text-amber-600";
  return "text-red-600";
};

const rubberStampColor = (pct: number): string => {
  if (pct > 30) return "text-red-600";
  if (pct > 20) return "text-amber-600";
  return "text-emerald-600";
};

const SortHeader = ({
  field,
  current,
  dir,
  onSort,
  children,
  className,
}: {
  field: SortField;
  current: SortField;
  dir: SortDir;
  onSort: (field: SortField) => void;
  children: React.ReactNode;
  className?: string;
}): React.ReactElement => (
  <TableHead className={className}>
    <button className="flex items-center gap-1 text-left font-medium" onClick={() => onSort(field)}>
      {children}
      <ArrowUpDown
        className={`size-3 ${current === field ? "text-foreground" : "text-muted-foreground/50"}`}
      />
      {current === field && <span className="text-xs">{dir === "asc" ? "\u2191" : "\u2193"}</span>}
    </button>
  </TableHead>
);

export const TeamHealthGrid = ({
  teams,
}: {
  teams: TeamReviewComparison[];
}): React.ReactElement | null => {
  const [sortField, setSortField] = useState<SortField>("avgDepth");
  const [sortDir, setSortDir] = useState<SortDir>("desc");

  const toggleSort = (field: SortField): void => {
    if (sortField === field) {
      setSortDir(sortDir === "asc" ? "desc" : "asc");
    } else {
      setSortField(field);
      setSortDir("desc");
    }
  };

  const sorted = useMemo(() => {
    const items = [...teams];
    const dir = sortDir === "asc" ? 1 : -1;
    items.sort((a, b) => {
      switch (sortField) {
        case "name":
          return dir * a.teamName.localeCompare(b.teamName);
        case "reviewCount":
          return dir * (a.reviewCount - b.reviewCount);
        case "avgDepth":
          return dir * (a.avgDepth - b.avgDepth);
        case "rubberStampPct":
          return dir * (a.rubberStampPct - b.rubberStampPct);
        default:
          return 0;
      }
    });
    return items;
  }, [teams, sortField, sortDir]);

  // Only show teams with enough review data
  const teamsWithData = sorted.filter((t) => t.reviewCount >= 5);
  if (teamsWithData.length === 0) return null;

  return (
    <Card>
      <CardHeader>
        <div className="flex items-center gap-2">
          <UsersRound className="size-4 text-muted-foreground" />
          <CardTitle>Team Health</CardTitle>
        </div>
        <CardDescription>Review culture comparison across teams</CardDescription>
      </CardHeader>
      <CardContent className="overflow-x-auto p-0">
        <Table>
          <TableHeader>
            <TableRow>
              <SortHeader field="name" current={sortField} dir={sortDir} onSort={toggleSort}>
                Team
              </SortHeader>
              <SortHeader
                field="reviewCount"
                current={sortField}
                dir={sortDir}
                onSort={toggleSort}
                className="text-right"
              >
                Reviews
              </SortHeader>
              <SortHeader
                field="avgDepth"
                current={sortField}
                dir={sortDir}
                onSort={toggleSort}
                className="text-right"
              >
                Avg Depth
              </SortHeader>
              <SortHeader
                field="rubberStampPct"
                current={sortField}
                dir={sortDir}
                onSort={toggleSort}
                className="text-right"
              >
                Rubber-stamp
              </SortHeader>
              <TableHead className="w-48">Sentiment</TableHead>
            </TableRow>
          </TableHeader>
          <TableBody>
            {teamsWithData.map((t) => (
              <TableRow key={t.teamId} className="hover:bg-muted/50">
                <TableCell>
                  <Link
                    to={`/teams/${t.teamId}`}
                    className="font-medium underline-offset-4 hover:underline"
                  >
                    {t.teamName}
                  </Link>
                </TableCell>
                <TableCell className="text-right tabular-nums">{t.reviewCount}</TableCell>
                <TableCell
                  className={`text-right tabular-nums font-medium ${depthColor(t.avgDepth)}`}
                >
                  {t.avgDepth.toFixed(2)}
                </TableCell>
                <TableCell
                  className={`text-right tabular-nums font-medium ${rubberStampColor(t.rubberStampPct)}`}
                >
                  {Math.round(t.rubberStampPct)}%
                </TableCell>
                <TableCell>
                  <SentimentBar
                    constructive={t.constructiveCount}
                    neutral={t.neutralCount}
                    critical={t.criticalCount}
                    hostile={0}
                  />
                </TableCell>
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </CardContent>
    </Card>
  );
};
