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
import { PersonAccounts } from "@/views/admin/components/person-accounts";
import { PersonFields } from "@/views/admin/components/person-fields";
import { useCreatePerson } from "@/views/admin/hooks/use-person-management";
import { personDraft, validatePersonDraft } from "@/views/admin/lib/person-form";
import { useState } from "react";

import type { Team } from "@ps/api/gen/canonical/prism/v1/org_pb";
import { useListSources } from "@ps/hooks/use-config";

export const AddPersonDialog = ({
  teams,
  open,
  onOpenChange,
}: {
  teams: Team[];
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const [draft, setDraft] = useState(() => personDraft());
  const [validationError, setValidationError] = useState<string>();

  const sources = useListSources();
  const createPerson = useCreatePerson();

  const handleSubmit = (event: React.FormEvent): void => {
    event.preventDefault();
    if (createPerson.isPending) return;

    const error = validatePersonDraft(draft);
    setValidationError(error);
    if (error) return;

    createPerson.mutate(draft, { onSuccess: () => onOpenChange(false) });
  };

  return (
    <Dialog
      open={open}
      onOpenChange={(value) => {
        if (!createPerson.isPending) onOpenChange(value);
      }}
    >
      <DialogContent className="min-w-0 sm:max-w-xl">
        <form onSubmit={handleSubmit} noValidate className="min-w-0">
          <DialogHeader>
            <DialogTitle>Add person</DialogTitle>
            <DialogDescription>
              Create a colleague with optional accounts. Choose a team deliberately or leave them unassigned.
            </DialogDescription>
          </DialogHeader>
          <fieldset
            disabled={createPerson.isPending}
            className="mt-4 min-w-0 space-y-4 max-h-[min(60vh,calc(100dvh-18rem))] overflow-y-auto"
          >
            <PersonFields draft={draft} onChange={setDraft} teams={teams} />
            <PersonAccounts
              accounts={draft.accounts}
              onChange={(accounts) => setDraft({ ...draft, accounts })}
              sources={sources.data ?? []}
              sourceError={sources.error}
            />
            {(validationError || createPerson.error) && (
              <Alert variant="destructive" className="min-w-0 break-words">
                {validationError ?? createPerson.error?.message}
              </Alert>
            )}
          </fieldset>
          <DialogFooter className="mt-4">
            <DialogClose render={<Button type="button" variant="outline" disabled={createPerson.isPending} />}>
              Cancel
            </DialogClose>
            <Button type="submit" disabled={createPerson.isPending}>
              {createPerson.isPending ? "Creating..." : "Create person"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
