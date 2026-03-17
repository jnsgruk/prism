import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { fmtFloat, fmtPercent } from "@/lib/format-metrics";
import type { GetIndividualProfileResponse } from "@/lib/hooks/use-metrics";

export const PeerContextPanel = ({
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
