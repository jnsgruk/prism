import { cn } from "@ps/cn";

const categories = [
  { key: "significant", label: "Significant", color: "bg-emerald-600" },
  { key: "notable", label: "Notable", color: "bg-sky-500" },
  { key: "routine", label: "Routine", color: "bg-slate-400" },
] as const;

export const SignificanceBreakdown = ({
  significant,
  notable,
  routine,
  className,
}: {
  significant: number;
  notable: number;
  routine: number;
  className?: string;
}): React.ReactElement | null => {
  const counts = { significant, notable, routine };
  const total = significant + notable + routine;
  if (total === 0) return null;

  return (
    <div className={cn("space-y-2", className)}>
      <div className="flex h-2.5 overflow-hidden rounded-full">
        {categories.map(({ key, color }) => {
          const count = counts[key];
          if (count === 0) return null;
          const pct = (count / total) * 100;
          return <div key={key} className={cn(color, "h-full")} style={{ width: `${pct}%` }} />;
        })}
      </div>
      <div className="flex gap-3 text-xs">
        {categories.map(({ key, label, color }) => {
          const count = counts[key];
          return (
            <span key={key} className="flex items-center gap-1 text-muted-foreground">
              <span className={cn("inline-block size-2 rounded-full", color)} />
              <span className="font-medium tabular-nums">{count}</span> {label}
            </span>
          );
        })}
      </div>
    </div>
  );
};
