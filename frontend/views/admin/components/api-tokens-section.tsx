import { Alert } from "@/components/ui/alert";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Key } from "lucide-react";

import type { ApiTokenInfo } from "@ps/api/gen/canonical/prism/v1/admin_pb";
import { formatDateOnly, formatRelativeTime } from "@/lib/format";
import { CreateTokenDialog } from "@/views/admin/components/create-token-dialog";
import { RevokeTokenDialog } from "@/views/admin/components/revoke-token-dialog";
import { useListApiTokens } from "@/views/admin/hooks/use-admin";

const ApiTokensContent = ({ tokens }: { tokens: ApiTokenInfo[] }): React.ReactElement => {
  if (tokens.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
        <Key className="mb-3 size-10 text-muted-foreground" />
        <p className="mb-1 font-medium">No API Tokens</p>
        <p className="text-sm text-muted-foreground">
          Create a token to authenticate psctl or other API clients.
        </p>
      </div>
    );
  }

  return (
    <Table>
      <TableHeader>
        <TableRow>
          <TableHead>Name</TableHead>
          <TableHead>Created</TableHead>
          <TableHead>Last Used</TableHead>
          <TableHead className="w-12" />
        </TableRow>
      </TableHeader>
      <TableBody>
        {tokens.map((token) => (
          <TableRow key={token.tokenId}>
            <TableCell className="font-medium">{token.name}</TableCell>
            <TableCell className="text-muted-foreground">
              {formatDateOnly(token.createdAt)}
            </TableCell>
            <TableCell className="text-muted-foreground">
              {formatRelativeTime(token.lastUsedAt)}
            </TableCell>
            <TableCell>
              <RevokeTokenDialog tokenId={token.tokenId} tokenName={token.name} />
            </TableCell>
          </TableRow>
        ))}
      </TableBody>
    </Table>
  );
};

export const ApiTokensSection = (): React.ReactElement => {
  const { data: tokens, isLoading, isError, error } = useListApiTokens();

  const content = (): React.ReactElement => {
    if (isLoading) {
      return (
        <div className="space-y-3">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-full" />
        </div>
      );
    }
    if (isError) {
      return (
        <Alert variant="destructive">
          {error instanceof Error ? error.message : "Failed to load API tokens"}
        </Alert>
      );
    }
    return <ApiTokensContent tokens={tokens ?? []} />;
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium">API Tokens</h3>
        <CreateTokenDialog />
      </div>
      <Separator />
      {content()}
    </div>
  );
};
