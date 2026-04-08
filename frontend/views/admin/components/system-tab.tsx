import { Separator } from "@/components/ui/separator";

import { ApiTokensSection } from "./api-tokens-section";
import { ResetDataDialog } from "./reset-data-dialog";

export const SystemTab = (): React.ReactElement => (
  <div className="space-y-6 pt-4">
    <p className="text-sm text-muted-foreground">
      System-wide settings, API tokens, and destructive operations.
    </p>

    <ApiTokensSection />

    <div className="space-y-4">
      <div>
        <h3 className="text-sm font-medium">Danger Zone</h3>
        <Separator className="mt-2" />
      </div>
      <div className="flex items-center justify-between rounded-lg border border-destructive/30 p-4">
        <div>
          <p className="text-sm font-medium">Reset all data</p>
          <p className="text-sm text-muted-foreground">
            Permanently delete all contributions, teams, people, and metric snapshots. Source
            configurations will be preserved.
          </p>
        </div>
        <ResetDataDialog />
      </div>
    </div>
  </div>
);
