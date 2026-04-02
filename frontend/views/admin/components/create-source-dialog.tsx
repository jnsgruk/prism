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
import { Separator } from "@/components/ui/separator";
import { ArrowLeft, Plus } from "lucide-react";
import { useState } from "react";

import type { JsonObject } from "@bufbuild/protobuf";
import { useCreateSource, useSetSecret } from "@ps/hooks/use-config";
import { useTriggerTeamSync } from "@/lib/hooks/use-ingestion";

import { Platform } from "@ps/api/gen/canonical/prism/v1/common_pb";
import { BufferedSecretForm } from "@/views/admin/components/secret-form";
import { settingsForms } from "@/views/admin/components/source-settings-forms";
import { SECRET_KEYS_BY_TYPE, SOURCE_TYPES } from "@/views/admin/lib/source-types";

type Step = "basics" | "configure";

const hasConfigStep = (sourceType: string): boolean => {
  const hasSettings = sourceType in settingsForms;
  const hasSecrets = (SECRET_KEYS_BY_TYPE[sourceType] ?? []).length > 0;
  return hasSettings || hasSecrets;
};

export const CreateSourceDialog = (): React.ReactElement => {
  const createSource = useCreateSource();
  const setSecret = useSetSecret();
  const triggerTeamSync = useTriggerTeamSync();

  const [open, setOpen] = useState(false);
  const [step, setStep] = useState<Step>("basics");
  const [name, setName] = useState("");
  const [sourceType, setSourceType] = useState("github");
  const [settings, setSettings] = useState<JsonObject>({});
  const [secrets, setSecrets] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const reset = (): void => {
    setStep("basics");
    setName("");
    setSourceType("github");
    setSettings({});
    setSecrets({});
    setError(null);
    setCreating(false);
  };

  const handleOpenChange = (nextOpen: boolean): void => {
    setOpen(nextOpen);
    if (!nextOpen) reset();
  };

  const handleTypeChange = (type: string): void => {
    if (type === sourceType) return;
    setSourceType(type);
    setSettings({});
    setSecrets({});
  };

  const handleNext = (e: React.FormEvent): void => {
    e.preventDefault();
    if (hasConfigStep(sourceType)) {
      setStep("configure");
    } else {
      handleCreate();
    }
  };

  const handleCreate = async (): Promise<void> => {
    setError(null);
    setCreating(true);

    try {
      // Create the source with settings
      const settingsPayload = Object.keys(settings).length > 0 ? settings : undefined;
      const platformEnum =
        SOURCE_TYPES.find((t) => t.value === sourceType)?.platform ?? Platform.GITHUB;
      const result = await createSource.mutateAsync({
        sourceType: platformEnum,
        name,
        settings: settingsPayload,
      });

      // Set any buffered secrets
      const sourceId = result.source?.id;
      if (sourceId) {
        const secretEntries = Object.entries(secrets).filter(([, v]) => v.trim());
        for (const [secretKey, secretValue] of secretEntries) {
          await setSecret.mutateAsync({ sourceId, secretKey, secretValue });
        }
      }

      // Auto-trigger team sync for GitHub sources (fire-and-forget)
      if (sourceType === "github" && name) {
        triggerTeamSync.mutate(name);
      }

      handleOpenChange(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to create source");
      setCreating(false);
    }
  };

  const SettingsForm = settingsForms[sourceType];
  const secretKeys = SECRET_KEYS_BY_TYPE[sourceType] ?? [];

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogTrigger render={<Button />}>
        <Plus className="size-4" />
        Add Source
      </DialogTrigger>
      <DialogContent className="max-w-lg">
        {step === "basics" ? (
          <form onSubmit={handleNext}>
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
                <Select value={sourceType} onValueChange={(v) => v !== null && handleTypeChange(v)}>
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

              {error && <Alert variant="destructive">{error}</Alert>}
            </div>

            <DialogFooter className="mt-4">
              <DialogClose render={<Button variant="outline" />}>Cancel</DialogClose>
              <Button type="submit" disabled={creating || !name.trim()}>
                {hasConfigStep(sourceType) ? "Next" : "Create"}
              </Button>
            </DialogFooter>
          </form>
        ) : (
          <div>
            <DialogHeader>
              <DialogTitle>Configure {name}</DialogTitle>
              <DialogDescription>
                Set up settings and credentials for your{" "}
                {SOURCE_TYPES.find((t) => t.value === sourceType)?.label ?? sourceType} source.
              </DialogDescription>
            </DialogHeader>

            <div className="mt-4 max-h-[60vh] space-y-4 overflow-y-auto">
              {SettingsForm && <SettingsForm settings={settings} onChange={setSettings} />}

              {SettingsForm && secretKeys.length > 0 && <Separator />}

              {secretKeys.length > 0 && (
                <div>
                  <p className="mb-3 text-sm font-medium">Credentials</p>
                  <BufferedSecretForm
                    sourceType={sourceType}
                    secrets={secrets}
                    onSecretsChange={setSecrets}
                  />
                </div>
              )}
            </div>

            {error && (
              <Alert variant="destructive" className="mt-4">
                {error}
              </Alert>
            )}

            <DialogFooter className="mt-4">
              <Button
                type="button"
                variant="outline"
                onClick={() => setStep("basics")}
                disabled={creating}
              >
                <ArrowLeft className="size-4" />
                Back
              </Button>
              <Button type="button" onClick={handleCreate} disabled={creating}>
                {creating ? "Creating..." : "Create"}
              </Button>
            </DialogFooter>
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
};
