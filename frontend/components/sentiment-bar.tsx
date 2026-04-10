import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

import { cn } from "@ps/cn";

const segments = [
  { key: "constructive", label: "Constructive", color: "bg-emerald-500" },
  { key: "neutral", label: "Neutral", color: "bg-slate-400" },
  { key: "critical", label: "Critical", color: "bg-orange-400" },
  { key: "hostile", label: "Hostile", color: "bg-red-500" },
] as const;

export const SentimentBar = ({
  constructive,
  neutral,
  critical,
  hostile,
  className,
}: {
  constructive: number;
  neutral: number;
  critical: number;
  hostile: number;
  className?: string;
}): React.ReactElement | null => {
  const counts = { constructive, neutral, critical, hostile };
  const total = constructive + neutral + critical + hostile;
  if (total === 0) return null;

  return (
    <div className={cn("flex flex-col gap-1", className)}>
      <div className="flex h-2.5 overflow-hidden rounded-full">
        {segments.map(({ key, label, color }) => {
          const count = counts[key];
          if (count === 0) return null;
          const pct = (count / total) * 100;
          return (
            <Tooltip key={key}>
              <TooltipTrigger render={<div className={cn(color, "h-full")} style={{ width: `${pct}%` }} />} />
              <TooltipContent side="bottom" className="text-xs">
                {label}: {count} ({Math.round(pct)}%)
              </TooltipContent>
            </Tooltip>
          );
        })}
      </div>
      <div className="flex gap-2 text-[10px] text-muted-foreground">
        {segments.map(({ key, label, color }) => {
          const count = counts[key];
          if (count === 0) return null;
          return (
            <span key={key} className="flex items-center gap-1">
              <span className={cn("inline-block size-1.5 rounded-full", color)} />
              {label} {count}
            </span>
          );
        })}
      </div>
    </div>
  );
};
