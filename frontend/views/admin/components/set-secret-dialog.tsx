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
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useState } from "react";

import type { SourceConfig } from "@ps/api/gen/prism/v1/config_pb";
import { useSetSecret } from "@ps/hooks/use-config";

import { SECRET_KEYS_BY_TYPE } from "@/views/admin/lib/source-types";

export const SetSecretDialog = ({
  source,
  open,
  onOpenChange,
}: {
  source: SourceConfig;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const setSecret = useSetSecret();
  const secretKeys = SECRET_KEYS_BY_TYPE[source.sourceType] ?? ["api_token"];
  const [selectedKey, setSelectedKey] = useState(secretKeys[0] ?? "api_token");
  const [secretValue, setSecretValue] = useState("");

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    setSecret.mutate(
      { sourceId: source.id, secretKey: selectedKey, secretValue },
      {
        onSuccess: () => {
          onOpenChange(false);
          setSecretValue("");
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>Set Secret</DialogTitle>
            <DialogDescription>{source.name}</DialogDescription>
          </DialogHeader>

          <div className="mt-4 space-y-4">
            {secretKeys.length > 1 && (
              <div className="space-y-2">
                <Label htmlFor="secret-key">Secret Key</Label>
                <Select value={selectedKey} onValueChange={(v) => v !== null && setSelectedKey(v)}>
                  <SelectTrigger className="w-full">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {secretKeys.map((k) => (
                      <SelectItem key={k} value={k}>
                        {k}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            )}

            <div className="space-y-2">
              <Label htmlFor="secret-value">
                {secretKeys.length <= 1 ? `Value (${selectedKey})` : "Value"}
              </Label>
              <Input
                id="secret-value"
                type="password"
                value={secretValue}
                onChange={(e) => setSecretValue(e.target.value)}
                placeholder="Paste your token here"
                className="font-mono"
                required
              />
            </div>

            {setSecret.isError && (
              <Alert variant="destructive">
                {setSecret.error instanceof Error
                  ? setSecret.error.message
                  : "Failed to set secret"}
              </Alert>
            )}
          </div>

          <DialogFooter className="mt-4">
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={setSecret.isPending || !secretValue.trim()}>
              {setSecret.isPending ? "Saving..." : "Save Secret"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
