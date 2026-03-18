import { useCallback } from "react";
import { useSearchParams } from "react-router";

import { PageHeader } from "@/components/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Brain, Cog, Key, Plug, Settings, UserCog, Users } from "lucide-react";

import { AiSettingsTab } from "@/views/admin/components/ai-settings-tab";
import { ApiTokensTab } from "@/views/admin/components/api-tokens-tab";
import { HandlersTab } from "@/views/admin/components/handlers-tab";
import { PeopleTab } from "@/views/admin/components/people-tab";
import { SourcesTab } from "@/views/admin/components/sources-tab";
import { SystemTab } from "@/views/admin/components/system-tab";
import { TeamsTab } from "@/views/admin/components/teams-tab";

const VALID_TABS = new Set(["sources", "teams", "people", "tokens", "handlers", "ai", "system"]);
const DEFAULT_TAB = "sources";

const AdminPage = (): React.ReactElement => {
  const [searchParams, setSearchParams] = useSearchParams();
  const rawTab = searchParams.get("tab");
  const tab = rawTab && VALID_TABS.has(rawTab) ? rawTab : DEFAULT_TAB;

  const setTab = useCallback(
    (value: string | number | null) => {
      if (typeof value !== "string") return;
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (value === DEFAULT_TAB) next.delete("tab");
          else next.set("tab", value);
          return next;
        },
        { replace: true },
      );
    },
    [setSearchParams],
  );

  return (
    <>
      <PageHeader title="Admin" description="Manage sources, teams, and platform settings" />
      <div className="flex-1 p-6">
        <Tabs value={tab} onValueChange={setTab}>
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
            <TabsTrigger value="handlers">
              <Cog className="size-4" />
              Handlers
            </TabsTrigger>
            <TabsTrigger value="ai">
              <Brain className="size-4" />
              AI
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
          <TabsContent value="handlers">
            <HandlersTab />
          </TabsContent>
          <TabsContent value="ai">
            <AiSettingsTab />
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
