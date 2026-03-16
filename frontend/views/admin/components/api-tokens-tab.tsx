import { Alert } from "@/components/ui/alert";
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

import { formatDateOnly, formatRelativeTime } from "@/lib/format";
import { CreateTokenDialog } from "@/views/admin/components/create-token-dialog";
import { RevokeTokenDialog } from "@/views/admin/components/revoke-token-dialog";
import { useListApiTokens } from "@/views/admin/hooks/use-admin";

export const ApiTokensTab = (): React.ReactElement => {
  const { data: tokens, isLoading, isError, error } = useListApiTokens();

  if (isLoading) {
    return (
      <div className="space-y-3 pt-4">
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
        <Skeleton className="h-10 w-full" />
      </div>
    );
  }

  if (isError) {
    return (
      <Alert variant="destructive" className="mt-4">
        {error instanceof Error ? error.message : "Failed to load API tokens"}
      </Alert>
    );
  }

  return (
    <div className="space-y-4 pt-4">
      <div className="flex items-center justify-between">
        <p className="text-sm text-muted-foreground">
          API tokens authenticate CLI tools and external integrations.
        </p>
        <CreateTokenDialog />
      </div>

      {tokens && tokens.length > 0 ? (
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
      ) : (
        <div className="flex flex-col items-center justify-center rounded-lg border-2 border-dashed p-12">
          <Key className="mb-3 size-10 text-muted-foreground" />
          <p className="mb-1 font-medium">No API Tokens</p>
          <p className="text-sm text-muted-foreground">
            Create a token to authenticate psctl or other API clients.
          </p>
        </div>
      )}
    </div>
  );
};
