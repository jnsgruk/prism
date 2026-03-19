import { ArrowDown, ArrowUp } from "lucide-react";
import { cn } from "@ps/cn";

/**
 * Display a period-over-period delta value with directional arrow.
 *
 * @param delta - The numeric change (positive = increase).
 * @param format - How to display the value: "decimal" (e.g. +0.12), "percent" (e.g. +3%), "integer" (e.g. +15).
 * @param invert - If true, a positive delta is bad (e.g. rubber-stamp rate going up).
 * @param suffix - Optional suffix like "%" shown after the number.
 */
export const DeltaBadge = ({
  delta,
  format = "decimal",
  invert = false,
  suffix = "",
  className,
}: {
  delta: number;
  format?: "decimal" | "percent" | "integer";
  invert?: boolean;
  suffix?: string;
  className?: string;
}): React.ReactElement | null => {
  if (delta === 0) return null;

  const isPositive = delta > 0;
  // By default, positive = good (green). If inverted, positive = bad (red).
  const isGood = invert ? !isPositive : isPositive;

  const formatted = ((): string => {
    switch (format) {
      case "decimal":
        return Math.abs(delta).toFixed(2);
      case "percent":
        return `${Math.abs(Math.round(delta))}`;
      case "integer":
        return String(Math.abs(Math.round(delta)));
    }
  })();

  return (
    <span
      className={cn(
        "inline-flex items-center gap-0.5 text-[10px] tabular-nums font-medium",
        isGood ? "text-emerald-600" : "text-red-600",
        className,
      )}
    >
      {isPositive ? <ArrowUp className="size-2.5" /> : <ArrowDown className="size-2.5" />}
      {formatted}
      {suffix}
    </span>
  );
};
