import { Badge } from "@/components/ui/badge";
import { TableCell, TableRow } from "@/components/ui/table";
import { ChevronRight } from "lucide-react";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";

export const MetricsRow = ({
  metrics,
  teamType,
  teamTypeBadge,
  hasChildren,
  onSelect,
}: {
  metrics: TeamMetrics;
  teamType: string | undefined;
  teamTypeBadge: "default" | "secondary" | "outline" | "destructive" | undefined;
  hasChildren: boolean;
  onSelect: () => void;
}): React.ReactElement => (
  <TableRow className="cursor-pointer hover:bg-muted/50" onClick={onSelect}>
    <TableCell className="font-medium">
      <span className="flex items-center gap-2">
        {metrics.teamName}
        {hasChildren && <ChevronRight className="size-3 text-muted-foreground" />}
      </span>
    </TableCell>
    <TableCell>
      {teamType && teamTypeBadge && (
        <Badge variant={teamTypeBadge} className="text-[10px]">
          {teamType}
        </Badge>
      )}
    </TableCell>
    <TableCell>
      <Badge variant={metrics.throughput > 0 ? "default" : "secondary"}>{metrics.throughput}</Badge>
    </TableCell>
    <TableCell>
      {metrics.reviewTurnaroundP75Hours > 0
        ? `${metrics.reviewTurnaroundP75Hours.toFixed(1)}h`
        : "\u2014"}
    </TableCell>
    <TableCell>{metrics.memberCount}</TableCell>
  </TableRow>
);
