import { createClient } from "@connectrpc/connect";
import type { UseQueryResult, UseMutationResult } from "@tanstack/react-query";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import type {
  CompleteSetupResponse,
  GetCurrentUserResponse,
  LoginResponse,
  LogoutResponse,
} from "@ps/api/gen/canonical/prism/v1/auth_pb";
import { AuthService } from "@ps/api/gen/canonical/prism/v1/auth_pb";
import { transport } from "@ps/api/transport";
import { clearSessionToken, setSessionToken } from "@ps/session";

const authClient = createClient(AuthService, transport);

export const authKeys = {
  all: ["auth"] as const,
  setupStatus: (): readonly ["auth", "setupStatus"] => [...authKeys.all, "setupStatus"] as const,
  currentUser: (): readonly ["auth", "currentUser"] => [...authKeys.all, "currentUser"] as const,
};

export const useSetupStatus = (): UseQueryResult<boolean> =>
  useQuery({
    queryKey: authKeys.setupStatus(),
    queryFn: () => authClient.getSetupStatus({}),
    select: (data): boolean => data.setupComplete,
  });

export const useCurrentUser = (): UseQueryResult<GetCurrentUserResponse> =>
  useQuery({
    queryKey: authKeys.currentUser(),
    queryFn: () => authClient.getCurrentUser({}),
    retry: false,
  });

export const useCompleteSetup = (): UseMutationResult<
  CompleteSetupResponse,
  Error,
  { username: string; displayName: string; password: string }
> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { username: string; displayName: string; password: string }) => authClient.completeSetup(req),
    onSuccess: (data) => {
      setSessionToken(data.sessionToken);
      queryClient.invalidateQueries({ queryKey: authKeys.all });
    },
  });
};

export const useLogin = (): UseMutationResult<LoginResponse, Error, { username: string; password: string }> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { username: string; password: string }) => authClient.login(req),
    onSuccess: (data) => {
      setSessionToken(data.sessionToken);
      queryClient.invalidateQueries({ queryKey: authKeys.currentUser() });
    },
  });
};

export const useLogout = (): UseMutationResult<LogoutResponse, Error, void> => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => authClient.logout({}),
    onSettled: () => {
      clearSessionToken();
      queryClient.clear();
    },
  });
};
