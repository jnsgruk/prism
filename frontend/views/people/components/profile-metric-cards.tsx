import { Activity, BarChart3, TrendingUp, Users } from "lucide-react";

import { fmtPercent } from "@/lib/format-metrics";
import type { GetIndividualProfileResponse } from "@/lib/hooks/use-metrics";

import { MetricCard } from "@/views/people/components/metric-card";

export const ProfileMetricCards = ({
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
