import { Progress as ProgressPrimitive } from "@base-ui/react/progress";

import { cn } from "@ps/cn";

const Progress = ({
  className,
  children,
  value,
  ...props
}: ProgressPrimitive.Root.Props): React.ReactElement => (
  <ProgressPrimitive.Root
    value={value}
    data-slot="progress"
    className={cn("flex flex-wrap gap-3", className)}
    {...props}
  >
    {children}
    <ProgressTrack>
      <ProgressIndicator />
    </ProgressTrack>
  </ProgressPrimitive.Root>
);

const ProgressTrack = ({
  className,
  ...props
}: ProgressPrimitive.Track.Props): React.ReactElement => (
  <ProgressPrimitive.Track
    className={cn(
      "relative flex h-1 w-full items-center overflow-x-hidden rounded-full bg-muted",
      className,
    )}
    data-slot="progress-track"
    {...props}
  />
);

const ProgressIndicator = ({
  className,
  ...props
}: ProgressPrimitive.Indicator.Props): React.ReactElement => (
  <ProgressPrimitive.Indicator
    data-slot="progress-indicator"
    className={cn("h-full bg-primary transition-all", className)}
    {...props}
  />
);

const ProgressLabel = ({
  className,
  ...props
}: ProgressPrimitive.Label.Props): React.ReactElement => (
  <ProgressPrimitive.Label
    className={cn("text-sm font-medium", className)}
    data-slot="progress-label"
    {...props}
  />
);

const ProgressValue = ({
  className,
  ...props
}: ProgressPrimitive.Value.Props): React.ReactElement => (
  <ProgressPrimitive.Value
    className={cn("ml-auto text-sm text-muted-foreground tabular-nums", className)}
    data-slot="progress-value"
    {...props}
  />
);

export { Progress, ProgressTrack, ProgressIndicator, ProgressLabel, ProgressValue };
