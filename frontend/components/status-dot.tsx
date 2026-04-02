import { cn } from "@ps/cn";

export const stateStyles: Record<string, { color: string; label: string }> = {
  collecting: { color: "bg-blue-500", label: "Collecting" },
  waiting: { color: "bg-amber-500", label: "Waiting" },
  idle: { color: "bg-emerald-500", label: "Idle" },
  error: { color: "bg-destructive", label: "Error" },
  running: { color: "bg-blue-500", label: "Running" },
  pending: { color: "bg-muted-foreground/40", label: "Pending" },
};

export const StatusDot = ({
  state,
  animate,
}: {
  state: string;
  animate: boolean;
}): React.ReactElement => {
  const color = stateStyles[state]?.color ?? "bg-emerald-500";
  return (
    <span className="relative flex size-2.5">
      {animate && (
        <span
          className={cn(
            "absolute inline-flex size-full animate-ping rounded-full opacity-75",
            color,
          )}
        />
      )}
      <span className={cn("relative inline-flex size-2.5 rounded-full", color)} />
    </span>
  );
};
