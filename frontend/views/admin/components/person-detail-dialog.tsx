import { Alert } from "@/components/ui/alert";
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
import { Separator } from "@/components/ui/separator";
import { PersonAccounts } from "@/views/admin/components/person-accounts";
import { PersonFields } from "@/views/admin/components/person-fields";
import { useDeactivatePerson, useReactivatePerson } from "@/views/admin/hooks/use-admin";
import { useSavePerson } from "@/views/admin/hooks/use-person-management";
import { personDraft, validatePersonDraft } from "@/views/admin/lib/person-form";
import { useRef, useState } from "react";

import type { Person, Team } from "@ps/api/gen/canonical/prism/v1/org_pb";
import { useListSources } from "@ps/hooks/use-config";

export const PersonDetailDialog = ({
  person,
  teams,
  open,
  onOpenChange,
}: {
  person: Person;
  teams: Team[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const baseline = useRef(person);
  const [draft, setDraft] = useState(() => personDraft(person));
  const [validationError, setValidationError] = useState<string>();

  const sources = useListSources();
  const save = useSavePerson();
  const deactivate = useDeactivatePerson();
  const reactivate = useReactivatePerson();

  const isPending = save.isPending || deactivate.isPending || reactivate.isPending;
  const error = validationError ?? save.error?.message ?? deactivate.error?.message ?? reactivate.error?.message;

  const handleSubmit = (event: React.FormEvent): void => {
    event.preventDefault();
    if (isPending) return;

    const validation = validatePersonDraft(draft);
    setValidationError(validation);
    if (validation) return;

    save.mutate(
      {
        baseline: baseline.current,
        draft,
        onProgress: (saved, added) => {
          baseline.current = saved;

          if (added)
            setDraft((current) => ({
              ...current,
              accounts: current.accounts.map((account) =>
                account.key === added.key ? { ...account, id: added.id } : account,
              ),
            }));
        },
      },
      { onSuccess: () => onOpenChange(false) },
    );
  };

  const toggleActive = (): void => {
    const mutation = person.active ? deactivate : reactivate;
    mutation.mutate(person.id, { onSuccess: () => onOpenChange(false) });
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!isPending) onOpenChange(value);
      }}
    >
      <DialogContent className="min-w-0 sm:max-w-xl">
        <form onSubmit={handleSubmit} noValidate className="min-w-0">
          <DialogHeader>
            <DialogTitle>{person.name}</DialogTitle>
            <DialogDescription>
              Edit details, accounts, team assignment, and status.
              {!person.active && " This person is currently inactive."}
            </DialogDescription>
          </DialogHeader>
          <fieldset
            disabled={isPending}
            className="mt-4 min-w-0 space-y-4 max-h-[min(60vh,calc(100dvh-18rem))] overflow-y-auto"
          >
            <PersonFields draft={draft} onChange={setDraft} teams={teams} />
            <Separator />
            <PersonAccounts
              accounts={draft.accounts}
              onChange={(accounts) => setDraft({ ...draft, accounts })}
              sources={sources.data ?? []}
              sourceError={sources.error}
            />
            <Separator />
            <div className="flex items-center justify-between gap-3">
              <div className="min-w-0">
                <p className="text-sm font-medium">{person.active ? "Deactivate" : "Reactivate"}</p>
                <p className="text-sm text-muted-foreground">
                  {person.active
                    ? "Remove this person from active reporting."
                    : "Restore this person to active status."}
                </p>
              </div>
              <Button
                type="button"
                variant={person.active ? "destructive" : "outline"}
                size="sm"
                onClick={toggleActive}
                disabled={isPending}
              >
                {person.active ? "Deactivate" : "Reactivate"}
              </Button>
            </div>
            {error && (
              <Alert variant="destructive" className="min-w-0 break-words">
                {error}
              </Alert>
            )}
          </fieldset>
          <DialogFooter className="mt-4">
            <DialogClose render={<Button type="button" variant="outline" disabled={isPending} />}>Cancel</DialogClose>
            <Button type="submit" disabled={isPending}>
              {isPending ? "Saving..." : "Save"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
