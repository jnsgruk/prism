import type { HandlerInfo } from "@ps/api/gen/canonical/prism/v1/handlers_pb";

import { HandlerRow } from "./handler-row";

export const HandlerSection = ({
  title,
  description,
  handlers,
  onCancelRun,
  cancelPending,
}: {
  title: string;
  description: string;
  handlers: HandlerInfo[];
  onCancelRun: (runId: string) => void;
  cancelPending: boolean;
}): React.ReactElement => {
  const runningCount = handlers.filter((h) => !!h.activeRun).length;
  const idleCount = handlers.length - runningCount;

  const summary =
    runningCount === 0 ? "All idle" : `${String(runningCount)} running · ${String(idleCount)} idle`;

  return (
    <div className="rounded-lg border">
      {/* Section header */}
      <div className="border-b px-4 py-3">
        <div className="flex items-baseline justify-between">
          <div>
            <h3 className="text-sm font-semibold">{title}</h3>
            <p className="text-xs text-muted-foreground">{description}</p>
          </div>
          <span className="shrink-0 text-xs text-muted-foreground">{summary}</span>
        </div>
      </div>

      {/* Column headers */}
      <div className="grid grid-cols-[1rem_minmax(7rem,1fr)_minmax(0,2fr)_4.5rem_5rem] gap-x-3 border-b px-4 py-1.5 text-xs text-muted-foreground">
        <span />
        <span>Handler</span>
        <span className="hidden sm:block">Status</span>
        <span className="hidden text-right sm:block">State</span>
        <span className="text-right">Actions</span>
      </div>

      {/* Handler rows */}
      {handlers.map((h) => (
        <HandlerRow
          key={h.name}
          handler={h}
          onCancelRun={onCancelRun}
          cancelPending={cancelPending}
        />
      ))}
    </div>
  );
};
