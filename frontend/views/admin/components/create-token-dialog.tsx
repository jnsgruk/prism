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
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Check, Copy, Plus } from "lucide-react";
import { useState } from "react";

import { useCreateApiToken } from "@/views/admin/hooks/use-admin";

export const CreateTokenDialog = (): React.ReactElement => {
  const createToken = useCreateApiToken();
  const [name, setName] = useState("");
  const [open, setOpen] = useState(false);
  const [copied, setCopied] = useState(false);

  const handleCreate = (e: React.FormEvent): void => {
    e.preventDefault();
    if (!name.trim()) return;
    createToken.mutate(name.trim());
  };

  const handleCopy = async (token: string): Promise<void> => {
    await navigator.clipboard.writeText(token);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleOpenChange = (next: boolean): void => {
    setOpen(next);
    if (!next) {
      setName("");
      setCopied(false);
      createToken.reset();
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger render={<Button />}>
        <Plus className="size-4" />
        Create Token
      </DialogTrigger>
      <DialogContent>
        {createToken.isSuccess ? (
          <>
            <DialogHeader>
              <DialogTitle>Token Created</DialogTitle>
              <DialogDescription>
                Copy your API token now. It won&apos;t be shown again.
              </DialogDescription>
            </DialogHeader>

            <div className="mt-4 space-y-3">
              <Label>API Token</Label>
              <div className="flex gap-2">
                <Input
                  readOnly
                  value={createToken.data.token}
                  className="font-mono text-xs"
                  onClick={(e) => (e.target as HTMLInputElement).select()}
                />
                <Button
                  variant="outline"
                  size="icon"
                  className="shrink-0"
                  onClick={() => void handleCopy(createToken.data.token)}
                >
                  {copied ? <Check className="size-4" /> : <Copy className="size-4" />}
                </Button>
              </div>
              <p className="text-sm text-muted-foreground">
                Store this token securely. You can use it with psctl via the{" "}
                <code className="rounded bg-muted px-1 py-0.5 text-xs">PS_API_TOKEN</code>{" "}
                environment variable or the{" "}
                <code className="rounded bg-muted px-1 py-0.5 text-xs">--token</code> flag.
              </p>
            </div>

            <DialogFooter className="mt-4">
              <Button onClick={() => handleOpenChange(false)}>{copied ? "Done" : "Close"}</Button>
            </DialogFooter>
          </>
        ) : (
          <form onSubmit={handleCreate}>
            <DialogHeader>
              <DialogTitle>Create API Token</DialogTitle>
              <DialogDescription>
                Create a personal API token for use with psctl or the Prism API.
              </DialogDescription>
            </DialogHeader>

            <div className="mt-4 space-y-4">
              <div className="space-y-2">
                <Label htmlFor="token-name">Token Name</Label>
                <Input
                  id="token-name"
                  value={name}
                  onChange={(e) => setName(e.target.value)}
                  placeholder="e.g. psctl-laptop, ci-pipeline"
                  autoComplete="off"
                  autoFocus
                />
              </div>

              {createToken.isError && (
                <Alert variant="destructive">
                  {createToken.error instanceof Error
                    ? createToken.error.message
                    : "Failed to create token"}
                </Alert>
              )}
            </div>

            <DialogFooter className="mt-4">
              <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
              <Button type="submit" disabled={!name.trim() || createToken.isPending}>
                {createToken.isPending ? "Creating..." : "Create Token"}
              </Button>
            </DialogFooter>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
};
