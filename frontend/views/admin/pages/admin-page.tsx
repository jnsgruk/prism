import { PageHeader } from "@/components/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Key, Plug, Settings, UserCog, Users } from "lucide-react";

import { ApiTokensTab } from "@/views/admin/components/api-tokens-tab";
import { PeopleTab } from "@/views/admin/components/people-tab";
import { SourcesTab } from "@/views/admin/components/sources-tab";
import { SystemTab } from "@/views/admin/components/system-tab";
import { TeamsTab } from "@/views/admin/components/teams-tab";

const AdminPage = (): React.ReactElement => {
  return (
    <>
      <PageHeader title="Admin" description="Manage sources, teams, and platform settings" />
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
            <TabsTrigger value="people">
              <UserCog className="size-4" />
              People
            </TabsTrigger>
            <TabsTrigger value="tokens">
              <Key className="size-4" />
              API Tokens
            </TabsTrigger>
            <TabsTrigger value="system">
              <Settings className="size-4" />
              System
            </TabsTrigger>
          </TabsList>
          <TabsContent value="sources">
            <SourcesTab />
          </TabsContent>
          <TabsContent value="teams">
            <TeamsTab />
          </TabsContent>
          <TabsContent value="people">
            <PeopleTab />
          </TabsContent>
          <TabsContent value="tokens">
            <ApiTokensTab />
          </TabsContent>
          <TabsContent value="system">
            <SystemTab />
          </TabsContent>
        </Tabs>
      </div>
    </>
  );
};

export default AdminPage;
