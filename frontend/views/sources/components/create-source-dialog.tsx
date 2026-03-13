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
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Plus } from "lucide-react";
import { useState } from "react";

import { useCreateSource } from "@ps/hooks/use-config";

import { SOURCE_TYPES } from "@/views/sources/lib/source-types";

export const CreateSourceDialog = (): React.ReactElement => {
  const createSource = useCreateSource();
  const [name, setName] = useState("");
  const [sourceType, setSourceType] = useState("github");
  const [open, setOpen] = useState(false);

  const handleSubmit = (e: React.FormEvent): void => {
    e.preventDefault();
    createSource.mutate(
      { sourceType, name },
      {
        onSuccess: () => {
          setOpen(false);
          setName("");
          setSourceType("github");
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button />}>
        <Plus className="size-4" />
        Add Source
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={handleSubmit}>
          <DialogHeader>
            <DialogTitle>Add Source</DialogTitle>
            <DialogDescription>Connect a new data source to Prism.</DialogDescription>
          </DialogHeader>

          <div className="mt-4 space-y-4">
            <div className="space-y-2">
              <Label htmlFor="source-name">Name</Label>
              <Input
                id="source-name"
                value={name}
                onChange={(e) => setName(e.target.value)}
                placeholder="e.g. canonical/ubuntu"
                required
              />
            </div>

            <div className="space-y-2">
              <Label htmlFor="source-type">Type</Label>
              <Select value={sourceType} onValueChange={(v) => v !== null && setSourceType(v)}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {SOURCE_TYPES.map((t) => (
                    <SelectItem key={t.value} value={t.value}>
                      {t.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            {createSource.isError && (
              <Alert variant="destructive">
                {createSource.error instanceof Error
                  ? createSource.error.message
                  : "Failed to create source"}
              </Alert>
            )}
          </div>

          <DialogFooter className="mt-4">
            <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
            <Button type="submit" disabled={createSource.isPending || !name.trim()}>
              {createSource.isPending ? "Creating..." : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
};
