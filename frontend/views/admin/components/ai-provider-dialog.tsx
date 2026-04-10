import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Command, CommandEmpty, CommandInput, CommandItem, CommandList } from "@/components/ui/command";
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
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Separator } from "@/components/ui/separator";
import {
  useAiModels,
  useAiSettings,
  useRefreshModelCatalogue,
  useSetProviderSecret,
  useTestProvider,
  useUpdateAiSettings,
} from "@/lib/hooks/use-ai-settings";
import { aiProviderKey } from "@/lib/proto-display";
import { CheckCircle2, ChevronsUpDown, Loader2, XCircle } from "lucide-react";
import { useState } from "react";
import { toast } from "sonner";

import { AiProvider } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { AiModelInfo } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

const TASK_TYPES = [
  {
    key: "enrichment",
    label: "Enrichment",
    description: "High-volume metadata tagging",
    capability: "completion",
  },
  {
    key: "agentic",
    label: "Agentic",
    description: "Tool-use for natural language queries",
    capability: "tool_use",
  },
  {
    key: "embeddings",
    label: "Embeddings",
    description: "Vector generation for similarity",
    capability: "embeddings",
  },
  {
    key: "imageGeneration",
    label: "Image Generation",
    description: "Default model for generate_image tool",
    capability: "image_generation",
  },
] as const;

export const AiProviderDialog = ({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}): React.ReactElement => {
  const { data: settings } = useAiSettings();
  const updateSettings = useUpdateAiSettings();
  const setSecret = useSetProviderSecret();
  const testProvider = useTestProvider();
  const refreshCatalogue = useRefreshModelCatalogue();

  const provider = AiProvider.GOOGLE;
  const providerKey = aiProviderKey(provider);
  const isKeySet = !!settings?.providerSecretStatus[providerKey];

  const [secretValue, setSecretValue] = useState("");
  const [showSecretInput, setShowSecretInput] = useState(false);

  const secretButtonLabel = isKeySet ? "Change" : "Set key";

  const getTaskConfig = (key: string): { provider: AiProvider; model: string } => {
    if (!settings) return { provider: AiProvider.GOOGLE, model: "" };
    const cfg = settings[key as keyof typeof settings];
    if (cfg && typeof cfg === "object" && "provider" in cfg) {
      return cfg as unknown as { provider: AiProvider; model: string };
    }
    return { provider: AiProvider.GOOGLE, model: "" };
  };

  const handleSaveSecret = (): void => {
    setSecret.mutate(
      { provider, secretValue },
      {
        onSuccess: () => {
          toast.success("API key saved");
          setSecretValue("");
          setShowSecretInput(false);
        },
        onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to save"),
      },
    );
  };

  const handleTest = (): void => {
    testProvider.mutate(
      { provider },
      {
        onSuccess: (res) => {
          if (res.success) toast.success("Connection OK");
          else toast.error(res.errorMessage || "Connection failed");
        },
        onError: (err) => toast.error(err instanceof Error ? err.message : "Test failed"),
      },
    );
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>Google Gemini</DialogTitle>
          <DialogDescription>Configure API credentials and model routing.</DialogDescription>
        </DialogHeader>

        <div className="max-h-[60vh] space-y-6 overflow-y-auto py-2">
          {/* API Key */}
          <div className="space-y-3">
            <div>
              <Label className="text-sm font-medium">API Key</Label>
              <p className="text-xs text-muted-foreground">Encrypted at rest. The value is never displayed.</p>
            </div>
            <div className="flex items-center gap-2">
              {isKeySet ? (
                <Badge variant="default" className="gap-1 text-[10px] uppercase">
                  <CheckCircle2 className="size-3" />
                  Set
                </Badge>
              ) : (
                <Badge variant="outline" className="gap-1 text-[10px] uppercase">
                  <XCircle className="size-3" />
                  Not set
                </Badge>
              )}
              <div className="ml-auto flex items-center gap-2">
                {isKeySet && (
                  <Button variant="outline" size="sm" disabled={testProvider.isPending} onClick={handleTest}>
                    {testProvider.isPending && <Loader2 className="mr-1.5 size-3.5 animate-spin" />}
                    Test
                  </Button>
                )}
                <Button variant="outline" size="sm" onClick={() => setShowSecretInput(!showSecretInput)}>
                  {showSecretInput ? "Cancel" : secretButtonLabel}
                </Button>
              </div>
            </div>
            {showSecretInput && (
              <div className="flex items-center gap-2">
                <Input
                  type="password"
                  placeholder="Google Gemini API key"
                  value={secretValue}
                  onChange={(e) => setSecretValue(e.target.value)}
                  className="font-mono"
                />
                <Button size="sm" disabled={!secretValue || setSecret.isPending} onClick={handleSaveSecret}>
                  {setSecret.isPending && <Loader2 className="mr-1.5 size-3.5 animate-spin" />}
                  Save
                </Button>
              </div>
            )}
          </div>

          <Separator />

          {/* Model Routing */}
          <div className="space-y-3">
            <div className="flex items-center justify-between">
              <div>
                <Label className="text-sm font-medium">Model Routing</Label>
                <p className="text-xs text-muted-foreground">Which model handles each AI task.</p>
              </div>
              <Button
                variant="outline"
                size="sm"
                disabled={refreshCatalogue.isPending}
                onClick={() => {
                  refreshCatalogue.mutate(undefined, {
                    onSuccess: () => toast.success("Model catalogue refresh started"),
                    onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to refresh"),
                  });
                }}
              >
                {refreshCatalogue.isPending && <Loader2 className="mr-1.5 size-3.5 animate-spin" />}
                Refresh models
              </Button>
            </div>
            <div className="space-y-3">
              {TASK_TYPES.map((task) => {
                const config = getTaskConfig(task.key);
                return (
                  <div key={task.key} className="space-y-1">
                    <div>
                      <p className="text-sm">{task.label}</p>
                      <p className="text-xs text-muted-foreground">{task.description}</p>
                    </div>
                    <ModelCombobox
                      provider={config.provider}
                      capability={task.capability}
                      value={config.model}
                      onSelect={(model) => {
                        updateSettings.mutate(
                          { [task.key]: { provider: config.provider, model } },
                          {
                            onSuccess: () => toast.success("Model updated"),
                            onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to update"),
                          },
                        );
                      }}
                      disabled={updateSettings.isPending}
                    />
                  </div>
                );
              })}
            </div>
          </div>
        </div>

        <DialogFooter>
          <DialogClose render={<Button variant="outline" />}>Close</DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};

// ---------------------------------------------------------------------------
// Model Combobox
// ---------------------------------------------------------------------------

/** Format a context length like "1M" or "128K". */
const formatContext = (n: number): string => {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n % 1_000 === 0 ? 0 : 1)}K`;
  return String(n);
};

/** Format price per million tokens. */
const formatPrice = (p: number | undefined): string => {
  if (p == null) return "—";
  return `$${p < 0.01 ? p.toFixed(3) : p.toFixed(2)}`;
};

const ModelCombobox = ({
  provider,
  capability,
  value,
  onSelect,
  disabled,
}: {
  provider: AiProvider;
  capability: string;
  value: string;
  onSelect: (modelId: string) => void;
  disabled: boolean;
}): React.ReactElement => {
  const [open, setOpen] = useState(false);
  const { data: modelsResponse } = useAiModels(provider, capability);

  const models = modelsResponse?.models ?? [];
  const selected = models.find((m) => m.id === value);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <button
            type="button"
            disabled={disabled}
            className="inline-flex h-9 w-full items-center justify-between rounded-md border border-input bg-transparent px-3 py-1 text-left text-sm shadow-xs transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
          />
        }
      >
        <span className="min-w-0 truncate text-sm">{selected ? selected.displayName : value || "Select model..."}</span>
        <ChevronsUpDown className="size-3.5 shrink-0 text-muted-foreground" />
      </PopoverTrigger>
      <PopoverContent className="w-[340px] p-0" align="start">
        <Command shouldFilter>
          <CommandInput placeholder="Search models..." />
          <CommandList>
            <CommandEmpty>
              {models.length === 0 ? "No models cached. Click Refresh models." : "No matching models."}
            </CommandEmpty>
            {models.map((m) => (
              <ModelCommandItem
                key={m.id}
                model={m}
                isSelected={m.id === value}
                onSelect={() => {
                  onSelect(m.id);
                  setOpen(false);
                }}
              />
            ))}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
};

const ModelCommandItem = ({
  model,
  isSelected,
  onSelect,
}: {
  model: AiModelInfo;
  isSelected: boolean;
  onSelect: () => void;
}): React.ReactElement => (
  <CommandItem
    value={`${model.id} ${model.displayName}`}
    data-checked={isSelected ? "true" : undefined}
    onSelect={onSelect}
  >
    <span className="flex min-w-0 flex-col">
      <span className="truncate text-sm">{model.displayName || model.id}</span>
      <span className="truncate text-xs text-muted-foreground">
        {model.contextLength > 0 && `${formatContext(model.contextLength)} ctx`}
        {model.inputPricePerMillion != null && (
          <>
            {model.contextLength > 0 && " · "}
            {formatPrice(model.inputPricePerMillion)}/M in · {formatPrice(model.outputPricePerMillion)}
            /M out
          </>
        )}
      </span>
    </span>
  </CommandItem>
);
