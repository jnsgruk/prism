import { Key } from "lucide-react";

export const ApiTokensTab = (): React.ReactElement => (
  <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
    <Key className="mb-3 size-10 text-muted-foreground" />
    <p className="mb-1 font-medium">API Tokens</p>
    <p className="text-sm text-muted-foreground">
      API token management will be implemented in a future workstream.
    </p>
  </div>
);
