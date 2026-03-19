import { cn } from "@ps/cn";

const DEPTH_LABELS = ["1", "2", "3", "4", "5"];

const depthColor = (score: number): string => {
  if (score <= 1) return "bg-red-500";
  if (score <= 2) return "bg-orange-400";
  if (score === 3) return "bg-yellow-400";
  if (score === 4) return "bg-emerald-400";
  return "bg-emerald-600";
};

export const DepthHistogram = ({
  distribution,
  className,
}: {
  distribution: number[];
  className?: string;
}): React.ReactElement => {
  const max = Math.max(...distribution, 1);

  return (
    <div className={cn("flex items-end gap-1.5", className)}>
      {distribution.map((count, i) => {
        const heightPct = (count / max) * 100;
        return (
          <div key={DEPTH_LABELS[i]} className="flex flex-1 flex-col items-center gap-1">
            <span className="text-[10px] tabular-nums text-muted-foreground">{count}</span>
            <div className="relative w-full" style={{ height: 48 }}>
              <div
                className={cn("absolute bottom-0 w-full rounded-t", depthColor(i + 1))}
                style={{ height: `${Math.max(heightPct, 4)}%` }}
              />
            </div>
            <span className="text-[10px] font-medium text-muted-foreground">{DEPTH_LABELS[i]}</span>
          </div>
        );
      })}
    </div>
  );
};
