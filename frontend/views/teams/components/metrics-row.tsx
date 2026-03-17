import { Badge } from "@/components/ui/badge";
import { TableCell, TableRow } from "@/components/ui/table";
import { ChevronRight } from "lucide-react";
import { useNavigate } from "react-router";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";
import { fmtFloat, fmtHours } from "@/lib/format-metrics";

export const MetricsRow = ({
  metrics,
  hasChildren,
  showDiscourse = false,
}: {
  metrics: TeamMetrics;
  hasChildren: boolean;
  showDiscourse?: boolean;
}): React.ReactElement => {
  const navigate = useNavigate();
  return (
    <TableRow
      className="cursor-pointer hover:bg-muted/50"
      onClick={() => navigate(`/teams/${metrics.teamId}`)}
    >
      <TableCell className="font-medium">
        <span className="flex items-center gap-2">
          {metrics.teamName}
          {hasChildren && <ChevronRight className="size-3 text-muted-foreground" />}
        </span>
      </TableCell>
      <TableCell>
        <Badge variant={metrics.throughput > 0 ? "default" : "secondary"}>
          {metrics.throughput}
        </Badge>
      </TableCell>
      <TableCell>{fmtHours(metrics.reviewTurnaroundP75Hours)}</TableCell>
      <TableCell className="tabular-nums">{fmtHours(metrics.avgCycleTimeHours)}</TableCell>
      <TableCell className="tabular-nums">{fmtFloat(metrics.wipAvg)}</TableCell>
      <TableCell className="tabular-nums">{fmtHours(metrics.leadTimeHours)}</TableCell>
      {showDiscourse && (
        <TableCell className="tabular-nums">{metrics.discourseTopicsCreated || "\u2014"}</TableCell>
      )}
      {showDiscourse && (
        <TableCell className="tabular-nums">{metrics.discoursePosts || "\u2014"}</TableCell>
      )}
      {showDiscourse && (
        <TableCell className="tabular-nums">
          {metrics.discourseLikesGiven + metrics.discourseLikesReceived || "\u2014"}
        </TableCell>
      )}
      <TableCell>{metrics.memberCount}</TableCell>
    </TableRow>
  );
};
