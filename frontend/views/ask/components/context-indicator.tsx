import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";

import type { ContextUsage } from "@/views/ask/hooks/use-ask-question";

/** Format a token count like "45K" or "1.2M". */
const formatTokens = (n: number): string => {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n % 1_000 === 0 ? 0 : 1)}K`;
  return String(n);
};

const SIZE = 24;
const STROKE = 3;
const RADIUS = (SIZE - STROKE) / 2;
const CIRCUMFERENCE = 2 * Math.PI * RADIUS;

export const ContextIndicator = ({
  contextUsage,
  onCompact,
}: {
  contextUsage: ContextUsage;
  onCompact?: () => void;
}): React.ReactElement | null => {
  const { inputTokens, outputTokens, contextWindow } = contextUsage;
  if (contextWindow <= 0) return null;

  const used = inputTokens + outputTokens;
  const pct = Math.min(used / contextWindow, 1);
  const offset = CIRCUMFERENCE * (1 - pct);

  let colorClass = "text-foreground/40";
  if (pct > 0.9) colorClass = "text-destructive";
  else if (pct > 0.7) colorClass = "text-yellow-500";

  return (
    <TooltipProvider delay={200}>
      <Tooltip>
        <TooltipTrigger
          render={
            <button
              type="button"
              className={`inline-flex shrink-0 items-center justify-center rounded-full transition-colors hover:bg-accent ${colorClass}`}
              style={{ width: SIZE, height: SIZE }}
              onClick={onCompact}
              aria-label={`Context usage: ${Math.round(pct * 100)}%`}
            />
          }
        >
          <svg width={SIZE} height={SIZE} className="-rotate-90">
            <circle
              cx={SIZE / 2}
              cy={SIZE / 2}
              r={RADIUS}
              fill="none"
              className="stroke-muted"
              strokeWidth={STROKE}
            />
            <circle
              cx={SIZE / 2}
              cy={SIZE / 2}
              r={RADIUS}
              fill="none"
              stroke="currentColor"
              strokeWidth={STROKE}
              strokeDasharray={CIRCUMFERENCE}
              strokeDashoffset={offset}
              strokeLinecap="round"
            />
          </svg>
        </TooltipTrigger>
        <TooltipContent side="top" className="text-xs">
          <p>
            Context: {formatTokens(used)} / {formatTokens(contextWindow)} tokens (
            {Math.round(pct * 100)}%)
          </p>
          <p className="text-muted-foreground">
            Input: {formatTokens(inputTokens)} · Output: {formatTokens(outputTokens)}
          </p>
          {onCompact && <p className="text-muted-foreground">Click to compact</p>}
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
};
