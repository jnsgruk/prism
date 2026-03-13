import { createClient } from "@connectrpc/connect";
import type { UseMutationResult } from "@tanstack/react-query";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { AdminService, type ResetDataResponse } from "@ps/api/gen/prism/v1/admin_pb";
import { transport } from "@ps/api/transport";
import { configKeys } from "@ps/hooks/use-config";

import { orgKeys } from "@/views/teams/hooks/use-teams";

const adminClient = createClient(AdminService, transport);

export const useResetData = (): UseMutationResult<ResetDataResponse, Error, void> => {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: () => adminClient.resetData({ confirm: true }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: configKeys.sources() });
      queryClient.invalidateQueries({ queryKey: orgKeys.all });
      queryClient.invalidateQueries({ queryKey: ["metrics"] });
    },
  });
};
