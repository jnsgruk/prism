import { PageHeader } from "@/components/page-header";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Key, Plug } from "lucide-react";

import { ApiTokensTab } from "@/views/sources/components/api-tokens-tab";
import { CreateSourceDialog } from "@/views/sources/components/create-source-dialog";
import { SourcesTab } from "@/views/sources/components/sources-tab";

const SourcesPage = (): React.ReactElement => {
  return (
    <>
      <PageHeader
        title="Admin"
        description="Manage sources and platform settings"
        actions={<CreateSourceDialog />}
      />
      <div className="flex-1 p-6">
        <Tabs defaultValue="sources">
          <TabsList>
            <TabsTrigger value="sources">
              <Plug className="size-4" />
              Sources
            </TabsTrigger>
            <TabsTrigger value="tokens">
              <Key className="size-4" />
              API Tokens
            </TabsTrigger>
          </TabsList>
          <TabsContent value="sources">
            <SourcesTab />
          </TabsContent>
          <TabsContent value="tokens">
            <ApiTokensTab />
          </TabsContent>
        </Tabs>
      </div>
    </>
  );
};

export default SourcesPage;
