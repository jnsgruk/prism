import { Separator } from "@/components/ui/separator";

import { ApiTokensSection } from "./api-tokens-section";
import { ImportDirectoryDialog } from "./import-directory-dialog";
import { ImportJiraUsersDialog } from "./import-jira-users-dialog";
import { ResetDataDialog } from "./reset-data-dialog";

export const SystemTab = (): React.ReactElement => (
  <div className="space-y-6 pt-4">
    <p className="text-sm text-muted-foreground">
      System-wide settings, API tokens, data imports, and destructive operations.
    </p>

    <ApiTokensSection />

    <div className="space-y-4">
      <div>
        <h3 className="text-sm font-medium">Data Import</h3>
        <Separator className="mt-2" />
      </div>
      <div className="flex items-center justify-between rounded-lg border p-4">
        <div>
          <p className="text-sm font-medium">Import Directory</p>
          <p className="text-sm text-muted-foreground">
            Upload an HTML or JSON directory export to bulk-import people and teams.
          </p>
        </div>
        <ImportDirectoryDialog />
      </div>
      <div className="flex items-center justify-between rounded-lg border p-4">
        <div>
          <p className="text-sm font-medium">Import Jira Users</p>
          <p className="text-sm text-muted-foreground">
            Upload a Jira Cloud user CSV export to map Jira account IDs to people.
          </p>
        </div>
        <ImportJiraUsersDialog />
      </div>
    </div>

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
