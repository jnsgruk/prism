import { cn } from "@ps/cn";

export const Stat = ({
  label,
  value,
  icon,
  variant,
}: {
  label: string;
  value: string;
  icon?: React.ReactNode;
  variant?: "warning" | "danger";
}): React.ReactElement => (
  <span
    className={cn(
      "inline-flex items-center gap-1 text-xs tabular-nums",
      variant === "warning" && "text-amber-600 dark:text-amber-400",
      variant === "danger" && "text-destructive",
      !variant && "text-muted-foreground",
    )}
  >
    {icon}
    <span className="font-medium">{value}</span>
    <span className="text-muted-foreground">{label}</span>
  </span>
);

export const DOT_SEP = <span className="text-muted-foreground/50">·</span>;
