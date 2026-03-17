import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Loader2, Play } from "lucide-react";
import { toast } from "sonner";

import type { HandlerInfo } from "@ps/api/gen/prism/v1/handlers_pb";
import { useListSources } from "@ps/hooks/use-config";

import { useTriggerHandler } from "@/views/ingestion/hooks/use-ingestion";

const displayName = (name: string): string => name.replace("Handler", "");

export const TriggerHandlerDialog = ({
  handler,
  open,
  onOpenChange,
}: {
  handler: HandlerInfo;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const { data: sources } = useListSources();
  const trigger = useTriggerHandler();
  const [method, setMethod] = useState(handler.methods[0] ?? "");
  const [sourceName, setSourceName] = useState("");

  const needsSource = handler.requiresKey;

  const handleTrigger = (): void => {
    if ((needsSource && !sourceName) || !method) return;
    trigger.mutate(
      { handlerName: handler.name, method, key: sourceName },
      {
        onSuccess: (resp) => {
          toast.success(
            `Triggered ${displayName(handler.name)}.${method} (${resp.invocationId.slice(0, 12)}...)`,
          );
          onOpenChange(false);
        },
        onError: (err) => {
          toast.error(err instanceof Error ? err.message : "Failed to trigger handler");
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <DialogTitle>Trigger {displayName(handler.name)}</DialogTitle>
          <DialogDescription>{handler.description}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4">
          <div className="space-y-2">
            <label className="text-sm font-medium">Method</label>
            <Select value={method} onValueChange={(v) => v !== null && setMethod(v)}>
              <SelectTrigger className="w-full">
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

          {needsSource && (
            <div className="space-y-2">
              <label className="text-sm font-medium">Source</label>
              <Select value={sourceName} onValueChange={(v) => v !== null && setSourceName(v)}>
                <SelectTrigger className="w-full">
                  <SelectValue placeholder="Select a source..." />
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
        </div>

        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button
            onClick={handleTrigger}
            disabled={(needsSource && !sourceName) || !method || trigger.isPending}
          >
            {trigger.isPending ? (
              <Loader2 className="mr-1 size-4 animate-spin" />
            ) : (
              <Play className="mr-1 size-4" />
            )}
            Trigger
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
