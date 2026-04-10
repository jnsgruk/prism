import { Alert } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogClose,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { useRevokeApiToken } from "@/views/admin/hooks/use-admin";
import { Trash2 } from "lucide-react";
import { useState } from "react";

export const RevokeTokenDialog = ({
  tokenId,
  tokenName,
}: {
  tokenId: string;
  tokenName: string;
}): React.ReactElement => {
  const revokeToken = useRevokeApiToken();
  const [open, setOpen] = useState(false);

  const handleRevoke = (): void => {
    revokeToken.mutate(tokenId, {
      onSuccess: () => setOpen(false),
    });
  };

  const handleOpenChange = (next: boolean): void => {
    setOpen(next);
    if (!next) revokeToken.reset();
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger render={<Button variant="ghost" size="icon" />}>
        <Trash2 className="size-4" />
      </DialogTrigger>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Revoke API Token</DialogTitle>
          <DialogDescription>
            Are you sure you want to revoke <span className="font-medium text-foreground">{tokenName}</span>? Any
            applications using this token will lose access immediately.
          </DialogDescription>
        </DialogHeader>

        {revokeToken.isError && (
          <Alert variant="destructive" className="mt-4">
            {revokeToken.error instanceof Error ? revokeToken.error.message : "Failed to revoke token"}
          </Alert>
        )}

        <DialogFooter className="mt-4">
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button variant="destructive" onClick={handleRevoke} disabled={revokeToken.isPending}>
            {revokeToken.isPending ? "Revoking..." : "Revoke Token"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
