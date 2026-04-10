import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";

const statusConfig = (
  containerStatus: string,
  isStreaming: boolean,
): { color: string; label: string; animate: boolean } => {
  if (containerStatus === "active") return { color: "bg-green-500", label: "Connected", animate: false };
  if (isStreaming) return { color: "bg-yellow-500", label: "Starting...", animate: true };
  return { color: "bg-muted-foreground", label: "Disconnected", animate: false };
};

export const PodStatusIndicator = ({
  containerStatus,
  podName,
  podIp,
  isStreaming,
}: {
  containerStatus: string;
  podName?: string;
  podIp?: string;
  isStreaming?: boolean;
}): React.ReactElement => {
  const { color, label, animate } = statusConfig(containerStatus, isStreaming ?? false);

  return (
    <TooltipProvider delay={200}>
      <Tooltip>
        <TooltipTrigger
          render={
            <button
              type="button"
              className="inline-flex shrink-0 items-center justify-center rounded-full p-1 transition-colors hover:bg-accent"
              aria-label={`Agent: ${label}`}
            />
          }
        >
          <span className="relative flex size-2.5">
            {animate && (
              <span className={`absolute inline-flex size-full animate-ping rounded-full opacity-75 ${color}`} />
            )}
            <span className={`relative inline-flex size-2.5 rounded-full ${color}`} />
          </span>
        </TooltipTrigger>
        <TooltipContent side="top" className="text-xs">
          <p className="font-medium">Agent: {label}</p>
          {containerStatus === "active" && podName && (
            <div className="mt-1 space-y-0.5 text-muted-foreground">
              <p>Pod: {podName}</p>
              {podIp && <p>IP: {podIp}</p>}
            </div>
          )}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
};
