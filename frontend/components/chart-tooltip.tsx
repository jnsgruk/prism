import type { TooltipContentProps } from "recharts/types/component/Tooltip";

export const ChartTooltip = ({ active, payload, label }: TooltipContentProps): React.ReactElement | null => {
  if (!active || !payload?.length) return null;
  return (
    <div className="rounded-md border bg-popover px-3 py-2 text-xs text-popover-foreground shadow-md">
      <p className="mb-1 font-medium">{label}</p>
      {payload.map((entry) => (
        <p key={entry.name} className="text-muted-foreground">
          {entry.name}: {entry.value}
        </p>
      ))}
    </div>
  );
};

export const cursorStyle = { fill: "hsl(var(--muted))", opacity: 0.5 };
