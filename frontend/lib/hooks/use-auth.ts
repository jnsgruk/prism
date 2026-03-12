import { createClient } from "@connectrpc/connect";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { AuthService } from "@ps/api/gen/prism/v1/auth_pb";
import { transport } from "@ps/api/transport";
import { clearSessionToken, setSessionToken } from "@ps/session";

const authClient = createClient(AuthService, transport);

export const authKeys = {
  all: ["auth"] as const,
  setupStatus: () => [...authKeys.all, "setupStatus"] as const,
  currentUser: () => [...authKeys.all, "currentUser"] as const,
};

export const useSetupStatus = () =>
  useQuery({
    queryKey: authKeys.setupStatus(),
    queryFn: () => authClient.getSetupStatus({}),
    select: (data) => data.setupComplete,
  });

export const useCurrentUser = () =>
  useQuery({
    queryKey: authKeys.currentUser(),
    queryFn: () => authClient.getCurrentUser({}),
    retry: false,
  });

export const useCompleteSetup = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { username: string; displayName: string; password: string }) => authClient.completeSetup(req),
    onSuccess: (data) => {
      setSessionToken(data.sessionToken);
      queryClient.invalidateQueries({ queryKey: authKeys.all });
    },
  });
};

export const useLogin = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: (req: { username: string; password: string }) => authClient.login(req),
    onSuccess: (data) => {
      setSessionToken(data.sessionToken);
      queryClient.invalidateQueries({ queryKey: authKeys.currentUser() });
    },
  });
};

export const useLogout = () => {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: () => authClient.logout({}),
    onSettled: () => {
      clearSessionToken();
      queryClient.clear();
    },
  });
};
