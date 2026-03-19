import { RunButton } from "@/components/run-cancel-buttons";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Loader2, Play } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import type { HandlerInfo } from "@ps/api/gen/prism/v1/handlers_pb";
import { useListSources } from "@ps/hooks/use-config";

import { useTriggerHandler } from "@/views/ingestion/hooks/use-ingestion";

const displayName = (name: string): string => name.replace("Handler", "");

export const TriggerHandlerPopover = ({
  handler,
}: {
  handler: HandlerInfo;
}): React.ReactElement => {
  const trigger = useTriggerHandler();
  const { data: sources } = useListSources();
  const [open, setOpen] = useState(false);
  const [method, setMethod] = useState(handler.methods[0] ?? "");
  const [sourceName, setSourceName] = useState("");

  const needsSource = handler.requiresKey;
  const hasSingleMethod = handler.methods.length <= 1;
  const isSimple = !needsSource && hasSingleMethod;

  const handleTrigger = (m?: string, key?: string): void => {
    const finalMethod = m ?? method;
    const finalKey = key ?? (needsSource ? sourceName : undefined);

    if (!finalMethod || (needsSource && !finalKey)) return;

    trigger.mutate(
      { handlerName: handler.name, method: finalMethod, key: finalKey ?? "" },
      {
        onSuccess: (resp) => {
          toast.success(
            `Triggered ${displayName(handler.name)}.${finalMethod} (${resp.invocationId.slice(0, 12)}...)`,
          );
          setOpen(false);
        },
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : "Failed to trigger handler");
        },
      },
    );
  };

  // Simple handlers: single method, no key — trigger directly
  if (isSimple) {
    return (
      <RunButton onClick={() => handleTrigger(handler.methods[0])} isPending={trigger.isPending} />
    );
  }

  // Complex handlers: popover with method + optional source selection
  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <Button variant="ghost" size="sm" className="h-7">
            <Play className="size-3.5" />
            <span className="ml-1 hidden sm:inline">Run</span>
          </Button>
        }
      />
      <PopoverContent align="end" className="w-64 space-y-3 p-3">
        <p className="text-xs font-medium">Trigger {displayName(handler.name)}</p>

        {!hasSingleMethod && (
          <div className="space-y-1.5">
            <label className="text-xs text-muted-foreground">Method</label>
            <Select value={method} onValueChange={(v) => v !== null && setMethod(v)}>
              <SelectTrigger className="h-8 w-full text-xs">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {handler.methods.map((m) => (
                  <SelectItem key={m} value={m}>
                    {m}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}

        {needsSource && (
          <div className="space-y-1.5">
            <label className="text-xs text-muted-foreground">Source</label>
            <Select value={sourceName} onValueChange={(v) => v !== null && setSourceName(v)}>
              <SelectTrigger className="h-8 w-full text-xs">
                <SelectValue placeholder="Select source..." />
              </SelectTrigger>
              <SelectContent>
                {sources?.map((s) => (
                  <SelectItem key={s.id} value={s.name}>
                    {s.name}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}

        <Button
          size="sm"
          className="h-7 w-full"
          onClick={() => handleTrigger()}
          disabled={(needsSource && !sourceName) || !method || trigger.isPending}
        >
          {trigger.isPending ? (
            <Loader2 className="mr-1 size-3.5 animate-spin" />
          ) : (
            <Play className="mr-1 size-3.5" />
          )}
          Trigger
        </Button>
      </PopoverContent>
    </Popover>
  );
};
