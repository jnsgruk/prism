import { PageHeader } from "@/components/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Key, Plug, Users } from "lucide-react";

import { ApiTokensTab } from "@/views/admin/components/api-tokens-tab";
import { CreateSourceDialog } from "@/views/admin/components/create-source-dialog";
import { ResetDataDialog } from "@/views/admin/components/reset-data-dialog";
import { SourcesTab } from "@/views/admin/components/sources-tab";
import { TeamsTab } from "@/views/admin/components/teams-tab";

const AdminPage = (): React.ReactElement => {
  return (
    <>
      <PageHeader
        title="Admin"
        description="Manage sources, teams, and platform settings"
        actions={
          <div className="flex items-center gap-2">
            <ResetDataDialog />
            <CreateSourceDialog />
          </div>
        }
      />
      <div className="flex-1 p-6">
        <Tabs defaultValue="sources">
          <TabsList>
            <TabsTrigger value="sources">
              <Plug className="size-4" />
              Sources
            </TabsTrigger>
            <TabsTrigger value="teams">
              <Users className="size-4" />
              Teams
            </TabsTrigger>
            <TabsTrigger value="tokens">
              <Key className="size-4" />
              API Tokens
            </TabsTrigger>
          </TabsList>
          <TabsContent value="sources">
            <SourcesTab />
          </TabsContent>
          <TabsContent value="teams">
            <TeamsTab />
          </TabsContent>
          <TabsContent value="tokens">
            <ApiTokensTab />
          </TabsContent>
        </Tabs>
      </div>
    </>
  );
};

export default AdminPage;
