import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Separator } from "@/components/ui/separator";
import { PersonBackfillHistory, PersonBackfillProgress } from "@/views/admin/components/person-backfill-progress";
import {
  useCancelPersonBackfill,
  useLaunchPersonBackfill,
  usePersonBackfillCapabilities,
  usePersonBackfillHistory,
  usePersonBackfillStatus,
} from "@/views/admin/hooks/use-person-backfill";
import { backfillDateError, backfillSourceReason, todayDate } from "@/views/admin/lib/person-backfill";
import { Loader2 } from "lucide-react";
import { useRef, useState } from "react";

import type { Person } from "@ps/api/gen/canonical/prism/v1/org_pb";
import { useListSources } from "@ps/hooks/use-config";
import { isActivePipeline } from "@ps/pipeline-status";

export const PersonBackfillDialog = ({
  person,
  open,
  onOpenChange,
}: {
  person: Person;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const sources = useListSources();
  const capabilities = usePersonBackfillCapabilities(open);
  const history = usePersonBackfillHistory(person.id, open);
  const launch = useLaunchPersonBackfill(person.id);
  const cancel = useCancelPersonBackfill(person.id);
  const [selected, setSelected] = useState<string[]>([]);
  const [sinceDate, setSinceDate] = useState("");
  const [selectedPipeline, setSelectedPipeline] = useState<string>();
  const [validation, setValidation] = useState<string>();
  const submission = useRef<{ intent: string; id: string } | undefined>(undefined);

  const summaries = history.data ?? [];
  const pipelineId = selectedPipeline ?? summaries[0]?.pipeline?.id;
  const status = usePersonBackfillStatus(person.id, pipelineId, open);
  const summary = summaries.find((value) => value.pipeline?.id === pipelineId);
  const pipeline = status.data ?? summary?.pipeline;
  const acceptedId = launch.data?.pipelineId;
  const acceptedPipeline =
    summaries.find((value) => value.pipeline?.id === acceptedId)?.pipeline ??
    (pipeline?.id === acceptedId ? pipeline : undefined);
  const acceptedUnconfirmed = !!acceptedId && !acceptedPipeline;
  const active =
    acceptedUnconfirmed ||
    summaries.some((value) => isActivePipeline(value.pipeline?.status)) ||
    isActivePipeline(pipeline?.status);
  const available = capabilities.data?.enabled === true;
  const selectedEligible =
    selected.length > 0 &&
    selected.every((id) => {
      const source = sources.data?.find((value) => value.id === id);
      return source && !backfillSourceReason(person, source);
    });
  const error =
    validation ??
    (sinceDate ? backfillDateError(sinceDate) : undefined) ??
    launch.error?.message ??
    cancel.error?.message ??
    sources.error?.message ??
    capabilities.error?.message ??
    history.error?.message ??
    status.error?.message;

  const submit = (event: React.FormEvent): void => {
    event.preventDefault();
    if (launch.isPending) return;
    const dateError = backfillDateError(sinceDate);
    setValidation(dateError ?? (!selectedEligible ? "Select at least one eligible source." : undefined));
    if (dateError || !selectedEligible || !available || active) return;

    const sourceIds = selected.toSorted();
    const intent = JSON.stringify({ personId: person.id, sourceIds, sinceDate });
    if (submission.current?.intent !== intent) submission.current = { intent, id: crypto.randomUUID() };
    launch.mutate(
      { sourceIds, sinceDate, submissionId: submission.current.id },
      {
        onSuccess: (response) => {
          setSelectedPipeline(response.pipelineId);
          submission.current = undefined;
        },
      },
    );
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!launch.isPending) onOpenChange(value);
      }}
    >
      <DialogContent className="min-w-0 sm:max-w-2xl">
        <DialogHeader>
          <DialogTitle>Backfill activity for {person.name}</DialogTitle>
          <DialogDescription>
            Use saved accounts to collect history from selected sources. Save account changes before launching a
            backfill.
          </DialogDescription>
        </DialogHeader>
        <div className="min-w-0 max-h-[65vh] space-y-4 overflow-y-auto">
          <form id="person-backfill-form" onSubmit={submit} noValidate className="min-w-0 space-y-4">
            {!available && (
              <Alert>
                {capabilities.isLoading
                  ? "Checking backfill availability…"
                  : capabilities.data?.reason || "Person backfills are currently unavailable."}
              </Alert>
            )}
            <fieldset disabled={launch.isPending || active} className="min-w-0 space-y-3">
              <legend className="mb-2 text-sm font-medium">Sources</legend>
              {sources.isLoading && <p className="text-sm">Loading sources…</p>}
              {sources.data?.length === 0 && <p className="text-sm text-muted-foreground">No sources configured.</p>}
              {sources.data?.map((source) => {
                const reason = backfillSourceReason(person, source);
                return (
                  <div key={source.id} className="flex min-w-0 items-start gap-3">
                    <Checkbox
                      id={`backfill-source-${source.id}`}
                      checked={selected.includes(source.id)}
                      disabled={!!reason}
                      onCheckedChange={(checked) =>
                        setSelected((current) =>
                          checked
                            ? [...current.filter((id) => id !== source.id), source.id]
                            : current.filter((id) => id !== source.id),
                        )
                      }
                    />
                    <div className="min-w-0 space-y-1">
                      <Label htmlFor={`backfill-source-${source.id}`} className="break-words">
                        {source.name}
                        {source.platformInstance && ` (${source.platformInstance})`}
                      </Label>
                      {reason && <p className="text-xs text-muted-foreground">{reason}</p>}
                    </div>
                  </div>
                );
              })}
              <div className="space-y-2">
                <Label htmlFor="person-backfill-date">Start date</Label>
                <p className="text-xs text-muted-foreground">Collect activity from midnight UTC on this date.</p>
                <Input
                  id="person-backfill-date"
                  type="date"
                  required
                  max={todayDate()}
                  value={sinceDate}
                  onChange={(event) => {
                    setSinceDate(event.target.value);
                    setValidation(undefined);
                  }}
                />
              </div>
            </fieldset>
          </form>
          {error && (
            <Alert variant="destructive" className="break-words">
              {error}
            </Alert>
          )}
          {pipeline && (
            <>
              <Separator />
              <PersonBackfillProgress pipeline={pipeline} runs={summary?.runs ?? []} />
            </>
          )}
          {summaries.length > 0 && (
            <>
              <Separator />
              <PersonBackfillHistory summaries={summaries} onSelect={setSelectedPipeline} />
            </>
          )}
        </div>
        <DialogFooter>
          <Button type="button" variant="outline" disabled={launch.isPending} onClick={() => onOpenChange(false)}>
            Close
          </Button>
          {pipeline && isActivePipeline(pipeline.status) && (
            <Button
              type="button"
              variant="destructive"
              disabled={cancel.isPending || pipeline.status === "cancelling"}
              onClick={() => cancel.mutate(pipeline.id)}
            >
              {pipeline.status === "cancelling" ? "Cancellation requested" : "Cancel backfill"}
            </Button>
          )}
          <Button
            type="submit"
            form="person-backfill-form"
            disabled={launch.isPending || active || !available || !selectedEligible || !!backfillDateError(sinceDate)}
          >
            {launch.isPending && <Loader2 className="mr-1.5 size-4 animate-spin" />}
            {launch.isPending ? "Submitting…" : "Start backfill"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
