import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import { Key, Loader2, Plug, Settings2 } from "lucide-react";
import { toast } from "sonner";

import { AiProvider } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { aiProviderKey } from "@/lib/proto-display";
import { useAiSettings, useTestProvider } from "@/lib/hooks/use-ai-settings";

import { AiCostSection } from "@/views/admin/components/ai-cost-tab";
import { AiProviderDialog } from "@/views/admin/components/ai-provider-dialog";

export const AiSettingsTab = (): React.ReactElement => {
  const { data: settings, isLoading } = useAiSettings();
  const testProvider = useTestProvider();
  const [showDialog, setShowDialog] = useState(false);

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="size-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  const provider = AiProvider.GOOGLE;
  const isKeySet = !!settings?.providerSecretStatus[aiProviderKey(provider)];

  return (
    <div className="space-y-6 pt-4">
      <p className="text-sm text-muted-foreground">Configure AI providers and model routing.</p>

      <div className="space-y-3">
        <div className="flex items-center justify-between rounded-lg border px-4 py-3">
          <div className="flex items-center gap-3">
            <div>
              <p className="text-sm font-medium">Google Gemini</p>
              <div className="flex items-center gap-2">
                <Badge variant="secondary">Google</Badge>
                {isKeySet ? (
                  <span className="flex items-center gap-1 text-xs text-green-600">
                    <Key className="size-3" /> Configured
                  </span>
                ) : (
                  <span className="flex items-center gap-1 text-xs text-amber-600">
                    <Key className="size-3" /> Needs secret
                  </span>
                )}
              </div>
            </div>
          </div>

          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="icon-sm"
              onClick={() => setShowDialog(true)}
              title="Edit settings"
            >
              <Settings2 className="size-4" />
            </Button>
            <Button
              variant="ghost"
              size="icon-sm"
              disabled={!isKeySet || testProvider.isPending}
              onClick={() =>
                testProvider.mutate(
                  { provider },
                  {
                    onSuccess: (res) => {
                      if (res.success) toast.success("Connection OK");
                      else toast.error(res.errorMessage || "Connection failed");
                    },
                    onError: (err) =>
                      toast.error(err instanceof Error ? err.message : "Test failed"),
                  },
                )
              }
              title="Test connection"
            >
              {testProvider.isPending ? (
                <Loader2 className="size-4 animate-spin" />
              ) : (
                <Plug className="size-4" />
              )}
            </Button>
          </div>
        </div>
      </div>

      <AiProviderDialog open={showDialog} onOpenChange={setShowDialog} />

      <Separator />

      <AiCostSection />
    </div>
  );
};
