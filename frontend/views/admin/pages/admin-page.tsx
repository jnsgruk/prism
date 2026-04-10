import { PageHeader } from "@/components/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { AiSettingsTab } from "@/views/admin/components/ai-settings-tab";
import { OrgTab } from "@/views/admin/components/org-tab";
import { SourcesTab } from "@/views/admin/components/sources-tab";
import { SystemTab } from "@/views/admin/components/system-tab";
import { Brain, Building2, Plug, Settings } from "lucide-react";
import { useCallback } from "react";
import { useSearchParams } from "react-router";

const VALID_TABS = new Set(["sources", "org", "ai", "system"]);
const DEFAULT_TAB = "sources";

/** Backwards-compat: map old tab names to new ones. */
const TAB_ALIASES: Record<string, string> = { teams: "org", people: "org" };

const AdminPage = (): React.ReactElement => {
  const [searchParams, setSearchParams] = useSearchParams();
  const rawTab = searchParams.get("tab");
  const resolved = rawTab ? (TAB_ALIASES[rawTab] ?? rawTab) : null;
  const tab = resolved && VALID_TABS.has(resolved) ? resolved : DEFAULT_TAB;

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
            <TabsTrigger value="org">
              <Building2 className="size-4" />
              Organisation
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
          <TabsContent value="org">
            <OrgTab />
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
