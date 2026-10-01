import { orgKeys } from "@/lib/hooks/use-org";
import { identityInput, type AccountDraft, type PersonDraft } from "@/views/admin/lib/person-form";
import { createClient } from "@connectrpc/connect";
import type { UseMutationResult, UseQueryResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import {
  OrgService,
  type Person,
  type CreatePersonResponse,
  type LookupJiraAccountsResponse,
} from "@ps/api/gen/canonical/prism/v1/org_pb";
import { transport } from "@ps/api/transport";

const client = createClient(OrgService, transport);

export const useCreatePerson = (): UseMutationResult<CreatePersonResponse, Error, PersonDraft> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: (draft: PersonDraft) =>
      client.createPerson({
        name: draft.name.trim(),
        email: draft.email.trim() || undefined,
        level: draft.level.trim() || undefined,
        teamId: draft.teamId || undefined,
        identities: draft.accounts.map(identityInput),
      }),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: orgKeys.all });
      toast.success("Person created");
    },
    onError: (error) => toast.error(error.message),
  });
};

type SaveInput = {
  baseline: Person;
  draft: PersonDraft;
  onProgress: (person: Person, added?: { key: string; id: string }) => void;
};

const updateIdentity = (
  personId: string,
  account: AccountDraft,
  previousUserId: string,
): ReturnType<typeof client.updatePersonIdentity> => {
  const input: Parameters<typeof client.updatePersonIdentity>[0] = {
    personId,
    identityId: account.id,
    username: account.username.trim(),
  };
  if (account.platformUserId.trim() !== previousUserId) {
    input.platformUserIdChange = account.platformUserId.trim()
      ? { case: "platformUserId", value: account.platformUserId.trim() }
      : { case: "clearPlatformUserId", value: true };
  }
  return client.updatePersonIdentity(input);
};

export const useSavePerson = (): UseMutationResult<Person, Error, SaveInput> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async ({ baseline, draft, onProgress }: SaveInput) => {
      let saved = baseline;
      const progress = (person?: Person, added?: { key: string; id: string }): void => {
        if (!person) throw new Error("The server did not return the saved person. Refresh and retry.");
        saved = person;
        onProgress(person, added);
      };
      if (
        draft.name.trim() !== saved.name ||
        draft.email.trim() !== (saved.email ?? "") ||
        draft.level.trim() !== (saved.level ?? "")
      ) {
        const response = await client.updatePerson({
          personId: saved.id,
          name: draft.name.trim(),
          email: draft.email.trim(),
          level: draft.level.trim(),
        });
        progress(response.person);
      }
      // Saved row IDs drive explicit removals; omitted accounts are never silently replaced.
      for (const identity of baseline.identities) {
        if (!draft.accounts.some((account) => account.id === identity.id)) {
          const response = await client.removePersonIdentity({ personId: saved.id, identityId: identity.id });
          progress(response.person);
        }
      }
      for (const account of draft.accounts) {
        const existing = saved.identities.find((identity) => identity.id === account.id);
        if (existing) {
          if (
            account.username.trim() !== existing.username ||
            account.platformUserId.trim() !== (existing.platformUserId ?? "")
          ) {
            const response = await updateIdentity(saved.id, account, existing.platformUserId ?? "");
            progress(response.person);
          }
        } else {
          const response = await client.addPersonIdentity({ personId: saved.id, identity: identityInput(account) });
          const added = response.person?.identities.find(
            (identity) => !saved.identities.some((previous) => previous.id === identity.id),
          );
          if (!added) throw new Error("The server did not return the new account. Refresh and retry.");
          progress(response.person, { key: account.key, id: added.id });
        }
      }
      if (draft.teamId !== (saved.teamId ?? "")) {
        if (saved.teamId) {
          await client.removePersonFromTeam({ personId: saved.id, teamId: saved.teamId });
          progress({ ...saved, teamId: undefined, teamName: undefined });
        }
        if (draft.teamId) {
          await client.assignPersonToTeam({ personId: saved.id, teamId: draft.teamId });
          progress({ ...saved, teamId: draft.teamId });
        }
      }
      return saved;
    },
    onSuccess: () => toast.success("Person saved"),
    onError: (error) => toast.error(error.message),
    // Refresh even after partial success, while the dialog retains its draft and retry baseline.
    onSettled: () => {
      void queryClient.invalidateQueries({ queryKey: orgKeys.all });
      void queryClient.invalidateQueries({ queryKey: ["metrics"] });
      void queryClient.invalidateQueries({ queryKey: ["insights", "person"] });
    },
  });
};

export const useLookupJiraAccounts = (
  sourceId: string,
  query: string,
  startAt = 0,
): UseQueryResult<LookupJiraAccountsResponse> =>
  useQuery({
    queryKey: [...orgKeys.all, "jira-account-lookup", sourceId, query, startAt],
    queryFn: () => client.lookupJiraAccounts({ sourceId, query, startAt, maxResults: 20 }),
    enabled: !!sourceId && query.trim().length >= 2,
    retry: false,
  });
