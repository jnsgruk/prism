import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { fmtFloat, fmtPercent } from "@/lib/format-metrics";
import type { GetIndividualProfileResponse } from "@/lib/hooks/use-metrics";

const metricLabels: Record<string, string> = {
  throughput: "Throughput",
  review_depth: "Avg review depth",
  rubber_stamp_rate: "Rubber-stamp rate",
};

const formatMetricValue = (name: string, value: number): string => {
  if (name === "rubber_stamp_rate") return `${Math.round(value)}%`;
  if (name === "review_depth") return value.toFixed(2);
  return fmtFloat(value);
};

export const PeerContextPanel = ({ profile }: { profile: GetIndividualProfileResponse }): React.ReactElement | null => {
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
            <div key={name} className="flex items-center justify-between rounded-md border px-3 py-2">
              <span className="text-sm">{metricLabels[name] ?? name.replace(/_/g, " ")}</span>
              <div className="flex items-center gap-2">
                <span className="tabular-nums text-sm font-medium">{formatMetricValue(name, p.value)}</span>
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
