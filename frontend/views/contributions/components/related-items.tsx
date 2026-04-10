import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { useEmbeddingSimilar } from "@/lib/hooks/use-embeddings";
import { contributionTypeLabel, platformLabel } from "@/lib/proto-display";
import { Layers, Link2 } from "lucide-react";
import { Link } from "react-router";

import type { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { SimilarItem } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

const distanceLabel = (distance: number): { text: string; className: string } => {
  if (distance < 0.15) return { text: "Very similar", className: "text-green-600" };
  if (distance < 0.3) return { text: "Similar", className: "text-foreground" };
  return { text: "Somewhat related", className: "text-muted-foreground" };
};

const SimilarItemRow = ({ item }: { item: SimilarItem }): React.ReactElement => {
  const label = distanceLabel(item.distance);
  return (
    <Link
      to={`/contributions/${item.contributionId}`}
      className="flex items-center justify-between rounded-md px-3 py-2 hover:bg-muted/50"
    >
      <div className="min-w-0 flex-1">
        <p className="truncate text-sm font-medium">{item.title || "Untitled"}</p>
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="capitalize">{platformLabel(item.platform)}</span>
          <span>&middot;</span>
          <span>{contributionTypeLabel(item.contributionType)}</span>
          {item.state && (
            <>
              <span>&middot;</span>
              <span>{item.state}</span>
            </>
          )}
        </div>
      </div>
      <Badge variant="outline" className={`ml-2 text-xs ${label.className}`}>
        {label.text} &middot; {item.distance.toFixed(2)}
      </Badge>
    </Link>
  );
};

const CrossPlatformLink = ({ item }: { item: SimilarItem }): React.ReactElement => (
  <Link
    to={`/contributions/${item.contributionId}`}
    className="flex items-center gap-2 rounded-md px-3 py-2 hover:bg-muted/50"
  >
    <Link2 className="size-4 shrink-0 text-muted-foreground" />
    <div className="min-w-0 flex-1">
      <p className="text-sm">
        <span className="text-muted-foreground">Likely related: </span>
        <span className="font-medium">{item.title || "Untitled"}</span>
      </p>
      <div className="flex items-center gap-2 text-xs text-muted-foreground">
        <Badge variant="secondary" className="text-[10px] uppercase">
          {platformLabel(item.platform)}
        </Badge>
        <span>{contributionTypeLabel(item.contributionType)}</span>
        <span>&middot; {item.distance.toFixed(2)}</span>
      </div>
    </div>
  </Link>
);

export const RelatedItems = ({
  contributionId,
  currentPlatform,
}: {
  contributionId: string;
  currentPlatform?: Platform;
}): React.ReactElement | null => {
  const { data, isLoading } = useEmbeddingSimilar(contributionId, {
    limit: 10,
  });

  if (isLoading) return <Skeleton className="h-40 w-full" />;
  if (!data?.items.length) return null;

  const crossPlatform = data.items.filter(
    (item: SimilarItem) => item.distance < 0.2 && item.platform !== currentPlatform,
  );
  const similar = data.items.filter((item: SimilarItem) => !crossPlatform.includes(item));

  return (
    <div className="space-y-4">
      {crossPlatform.length > 0 && (
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="flex items-center gap-2 text-sm">
              <Link2 className="size-4" /> Cross-Platform Links
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-1">
            {crossPlatform.map((item: SimilarItem) => (
              <CrossPlatformLink key={item.contributionId} item={item} />
            ))}
          </CardContent>
        </Card>
      )}

      {similar.length > 0 && (
        <Card>
          <CardHeader className="pb-3">
            <CardTitle className="flex items-center gap-2 text-sm">
              <Layers className="size-4" /> Similar Contributions
              <Badge variant="secondary">{similar.length}</Badge>
            </CardTitle>
          </CardHeader>
          <CardContent className="space-y-1">
            {similar.map((item: SimilarItem) => (
              <SimilarItemRow key={item.contributionId} item={item} />
            ))}
          </CardContent>
        </Card>
      )}
    </div>
  );
};
