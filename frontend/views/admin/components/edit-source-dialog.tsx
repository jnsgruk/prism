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
import { Separator } from "@/components/ui/separator";
import { useEffect, useState } from "react";

import type { JsonObject } from "@bufbuild/protobuf";
import type { SourceConfig } from "@ps/api/gen/prism/v1/config_pb";
import { useUpdateSource } from "@ps/hooks/use-config";

import { SecretForm } from "@/views/admin/components/secret-form";
import { settingsForms } from "@/views/admin/components/source-settings-forms";
import { baseSourceType } from "@/views/admin/lib/source-types";

export const EditSourceDialog = ({
  source,
  open,
  onOpenChange,
}: {
  source: SourceConfig;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const updateSource = useUpdateSource();
  const [settings, setSettings] = useState<JsonObject>({});

  // Sync settings from source when dialog opens
  useEffect(() => {
    if (open) {
      setSettings(source.settings ?? {});
    }
  }, [open, source.settings]);

  const SettingsForm = settingsForms[baseSourceType(source.sourceType)];

  const handleSave = (): void => {
    updateSource.mutate(
      { sourceId: source.id, settings },
      {
        onSuccess: () => {
          onOpenChange(false);
        },
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="max-w-lg">
        <DialogHeader>
          <DialogTitle>{source.name}</DialogTitle>
          <DialogDescription>Configure source settings and credentials.</DialogDescription>
        </DialogHeader>

        <div className="max-h-[60vh] space-y-4 overflow-y-auto">
          {SettingsForm ? (
            <SettingsForm settings={settings} onChange={setSettings} />
          ) : (
            <p className="text-sm text-muted-foreground">
              No configurable settings for this source type.
            </p>
          )}

          <Separator />

          <div>
            <p className="mb-3 text-sm font-medium">Credentials</p>
            <SecretForm source={source} />
          </div>
        </div>

        {updateSource.isError && (
          <Alert variant="destructive">
            {updateSource.error instanceof Error
              ? updateSource.error.message
              : "Failed to update source"}
          </Alert>
        )}

        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
          <Button onClick={handleSave} disabled={updateSource.isPending}>
            {updateSource.isPending ? "Saving..." : "Save Settings"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};
