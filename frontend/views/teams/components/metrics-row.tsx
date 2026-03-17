import { Badge } from "@/components/ui/badge";
import { TableCell, TableRow } from "@/components/ui/table";
import { ChevronRight } from "lucide-react";
import { useNavigate } from "react-router";

import type { TeamMetrics } from "@ps/api/gen/prism/v1/metrics_pb";

const fmtHours = (h: number): string => (h > 0 ? `${h.toFixed(1)}h` : "\u2014");
const fmtFloat = (v: number): string => (v > 0 ? v.toFixed(1) : "\u2014");

export const MetricsRow = ({
  metrics,
  teamType,
  teamTypeBadge,
  hasChildren,
}: {
  metrics: TeamMetrics;
  teamType: string | undefined;
  teamTypeBadge: "default" | "secondary" | "outline" | "destructive" | undefined;
  hasChildren: boolean;
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
          {metrics.sourcePlatforms.length > 0 && (
            <span className="flex gap-0.5">
              {metrics.sourcePlatforms.map((p) => (
                <Badge key={p} variant="outline" className="px-1 py-0 text-[9px]">
                  {p}
                </Badge>
              ))}
            </span>
          )}
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
        <Badge variant={metrics.throughput > 0 ? "default" : "secondary"}>
          {metrics.throughput}
        </Badge>
      </TableCell>
      <TableCell>{fmtHours(metrics.reviewTurnaroundP75Hours)}</TableCell>
      <TableCell className="tabular-nums">{fmtHours(metrics.avgCycleTimeHours)}</TableCell>
      <TableCell className="tabular-nums">{fmtFloat(metrics.wipAvg)}</TableCell>
      <TableCell className="tabular-nums">{fmtHours(metrics.leadTimeHours)}</TableCell>
      <TableCell>{metrics.memberCount}</TableCell>
    </TableRow>
  );
};
