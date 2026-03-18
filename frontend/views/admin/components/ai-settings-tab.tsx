import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  Command,
  CommandEmpty,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Separator } from "@/components/ui/separator";
import {
  CheckCircle2,
  ChevronsUpDown,
  Database,
  Eye,
  EyeOff,
  Loader2,
  RefreshCw,
  XCircle,
} from "lucide-react";
import { toast } from "sonner";

import type { AiModelInfo } from "@ps/api/gen/prism/v1/reasoning_pb";

import { AiCostSection } from "@/views/admin/components/ai-cost-tab";
import {
  useAiModels,
  useAiSettings,
  useRefreshModelCatalogue,
  useSetProviderSecret,
  useStorageHealth,
  useTestProvider,
  useUpdateAiSettings,
} from "@/views/admin/hooks/use-ai-settings";

const PROVIDERS = [
  { value: "google", label: "Google Gemini" },
  { value: "openrouter", label: "OpenRouter" },
];

const TASK_TYPES = [
  {
    key: "enrichment",
    label: "Enrichment",
    description: "High-volume metadata tagging",
    capability: "completion",
  },
  {
    key: "insights",
    label: "Insights",
    description: "Deep reasoning for reports",
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
] as const;

export const AiSettingsTab = (): React.ReactElement => {
  const { data: settings, isLoading } = useAiSettings();
  const updateSettings = useUpdateAiSettings();
  const setSecret = useSetProviderSecret();
  const testProvider = useTestProvider();
  const { data: storageHealth } = useStorageHealth();
  const refreshCatalogue = useRefreshModelCatalogue();

  if (isLoading) {
    return (
      <div className="flex items-center justify-center py-12">
        <Loader2 className="size-6 animate-spin text-muted-foreground" />
      </div>
    );
  }

  return (
    <div className="space-y-6 pt-4">
      <p className="text-sm text-muted-foreground">
        Configure AI providers, model routing, and budget limits.
      </p>

      <ProviderCredentialsSection
        secretStatus={settings?.providerSecretStatus ?? {}}
        onSetSecret={(provider, value) => {
          setSecret.mutate(
            { provider, secretValue: value },
            {
              onSuccess: () => toast.success(`${provider} API key saved`),
              onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to save"),
            },
          );
        }}
        onTestProvider={(provider) => {
          testProvider.mutate(
            { provider },
            {
              onSuccess: (res) => {
                if (res.success) toast.success(`${provider} connection OK`);
                else toast.error(res.errorMessage || "Connection failed");
              },
              onError: (err) => toast.error(err instanceof Error ? err.message : "Test failed"),
            },
          );
        }}
        onRefreshModels={() => {
          refreshCatalogue.mutate(undefined, {
            onSuccess: () => toast.success("Model catalogue refresh started"),
            onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to refresh"),
          });
        }}
        isSettingSecret={setSecret.isPending}
        isTesting={testProvider.isPending}
        isRefreshing={refreshCatalogue.isPending}
      />

      <TaskRoutingSection
        settings={settings}
        onUpdate={(req) => {
          updateSettings.mutate(req, {
            onSuccess: () => toast.success("AI settings updated"),
            onError: (err) => toast.error(err instanceof Error ? err.message : "Failed to update"),
          });
        }}
        isUpdating={updateSettings.isPending}
      />

      <BudgetSection
        currentCap={settings?.budgetCapUsd}
        onUpdate={(cap) => {
          updateSettings.mutate(
            { budgetCapUsd: cap },
            {
              onSuccess: () => toast.success("Budget cap updated"),
              onError: (err) =>
                toast.error(err instanceof Error ? err.message : "Failed to update"),
            },
          );
        }}
        isUpdating={updateSettings.isPending}
      />

      <StorageHealthSection
        healthy={storageHealth?.healthy ?? false}
        errorMessage={storageHealth?.errorMessage}
      />

      <Separator className="my-2" />

      <AiCostSection />
    </div>
  );
};

// ---------------------------------------------------------------------------
// Provider Credentials
// ---------------------------------------------------------------------------

const ProviderCredentialsSection = ({
  secretStatus,
  onSetSecret,
  onTestProvider,
  onRefreshModels,
  isSettingSecret,
  isTesting,
  isRefreshing,
}: {
  secretStatus: Record<string, boolean>;
  onSetSecret: (provider: string, value: string) => void;
  onTestProvider: (provider: string) => void;
  onRefreshModels: () => void;
  isSettingSecret: boolean;
  isTesting: boolean;
  isRefreshing: boolean;
}): React.ReactElement => (
  <Card>
    <CardHeader>
      <div className="flex items-center justify-between">
        <div>
          <CardTitle className="text-base">Provider Credentials</CardTitle>
          <CardDescription>
            API keys are encrypted at rest. Values are never displayed.
          </CardDescription>
        </div>
        <Button variant="outline" size="sm" disabled={isRefreshing} onClick={onRefreshModels}>
          {isRefreshing ? (
            <Loader2 className="mr-1.5 size-3.5 animate-spin" />
          ) : (
            <RefreshCw className="mr-1.5 size-3.5" />
          )}
          Refresh models
        </Button>
      </div>
    </CardHeader>
    <CardContent className="space-y-4">
      {PROVIDERS.map((p) => (
        <ProviderKeyRow
          key={p.value}
          label={p.label}
          isSet={!!secretStatus[p.value]}
          onSave={(value) => onSetSecret(p.value, value)}
          onTest={() => onTestProvider(p.value)}
          isSaving={isSettingSecret}
          isTesting={isTesting}
        />
      ))}
    </CardContent>
  </Card>
);

const buttonLabel = (isSet: boolean): string => (isSet ? "Change" : "Set key");

const ProviderKeyRow = ({
  label,
  isSet,
  onSave,
  onTest,
  isSaving,
  isTesting,
}: {
  label: string;
  isSet: boolean;
  onSave: (value: string) => void;
  onTest: () => void;
  isSaving: boolean;
  isTesting: boolean;
}): React.ReactElement => {
  const [value, setValue] = useState("");
  const [showInput, setShowInput] = useState(false);

  return (
    <div className="flex items-center gap-3 rounded-lg border p-4">
      <div className="min-w-0 flex-1">
        <div className="flex items-center gap-2">
          <p className="text-sm font-medium">{label}</p>
          {isSet ? (
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
        </div>
        {showInput && (
          <div className="mt-2 flex items-center gap-2">
            <Input
              type="password"
              placeholder={`${label} API key`}
              value={value}
              onChange={(e) => setValue(e.target.value)}
              className="max-w-sm"
            />
            <Button
              size="sm"
              disabled={!value || isSaving}
              onClick={() => {
                onSave(value);
                setValue("");
                setShowInput(false);
              }}
            >
              {isSaving && <Loader2 className="mr-1.5 size-3.5 animate-spin" />}
              Save
            </Button>
          </div>
        )}
      </div>
      <div className="flex items-center gap-2">
        <Button variant="outline" size="sm" onClick={() => setShowInput(!showInput)}>
          {showInput ? <EyeOff className="mr-1.5 size-3.5" /> : <Eye className="mr-1.5 size-3.5" />}
          {showInput ? "Cancel" : buttonLabel(isSet)}
        </Button>
        {isSet && (
          <Button variant="outline" size="sm" disabled={isTesting} onClick={onTest}>
            {isTesting && <Loader2 className="mr-1.5 size-3.5 animate-spin" />}
            Test
          </Button>
        )}
      </div>
    </div>
  );
};

// ---------------------------------------------------------------------------
// Task Routing
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
  provider: string;
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
            className="inline-flex h-9 w-[260px] items-center justify-between rounded-md border border-input bg-transparent px-3 py-1 text-left text-sm shadow-xs transition-colors hover:bg-accent disabled:cursor-not-allowed disabled:opacity-50"
          />
        }
      >
        <span className="min-w-0 truncate text-sm">
          {selected ? selected.displayName : value || "Select model..."}
        </span>
        <ChevronsUpDown className="size-3.5 shrink-0 text-muted-foreground" />
      </PopoverTrigger>
      <PopoverContent className="w-[340px] p-0" align="start">
        <Command shouldFilter>
          <CommandInput placeholder="Search models..." />
          <CommandList>
            <CommandEmpty>
              {models.length === 0
                ? "No models cached. Click Refresh models above."
                : "No matching models."}
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
            {formatPrice(model.inputPricePerMillion)}/M in ·{" "}
            {formatPrice(model.outputPricePerMillion)}
            /M out
          </>
        )}
      </span>
    </span>
  </CommandItem>
);

const TaskRoutingSection = ({
  settings,
  onUpdate,
  isUpdating,
}: {
  settings: ReturnType<typeof useAiSettings>["data"];
  onUpdate: (req: Record<string, unknown>) => void;
  isUpdating: boolean;
}): React.ReactElement => {
  const getTaskConfig = (key: string): { provider: string; model: string } => {
    if (!settings) return { provider: "google", model: "" };
    const cfg = settings[key as keyof typeof settings];
    if (cfg && typeof cfg === "object" && "provider" in cfg) {
      return cfg as { provider: string; model: string };
    }
    return { provider: "google", model: "" };
  };

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Task Routing</CardTitle>
        <CardDescription>Which provider and model handles each AI task.</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="space-y-4">
          {TASK_TYPES.map((task) => {
            const config = getTaskConfig(task.key);
            return (
              <div key={task.key} className="flex items-center gap-4 rounded-lg border p-4">
                <div className="min-w-0 flex-1">
                  <p className="text-sm font-medium">{task.label}</p>
                  <p className="text-xs text-muted-foreground">{task.description}</p>
                </div>
                <Select
                  value={config.provider}
                  onValueChange={(provider) => {
                    onUpdate({ [task.key]: { provider, model: config.model } });
                  }}
                  disabled={isUpdating}
                >
                  <SelectTrigger className="w-[160px]">
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {PROVIDERS.map((p) => (
                      <SelectItem key={p.value} value={p.value}>
                        {p.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <ModelCombobox
                  provider={config.provider}
                  capability={task.capability}
                  value={config.model}
                  onSelect={(model) => {
                    onUpdate({ [task.key]: { provider: config.provider, model } });
                  }}
                  disabled={isUpdating}
                />
              </div>
            );
          })}
        </div>
      </CardContent>
    </Card>
  );
};

// ---------------------------------------------------------------------------
// Budget
// ---------------------------------------------------------------------------

const BudgetSection = ({
  currentCap,
  onUpdate,
  isUpdating,
}: {
  currentCap: number | undefined;
  onUpdate: (cap: number) => void;
  isUpdating: boolean;
}): React.ReactElement => {
  const [value, setValue] = useState("");

  return (
    <Card>
      <CardHeader>
        <CardTitle className="text-base">Daily Budget Cap</CardTitle>
        <CardDescription>
          Enrichment pauses when the daily spend exceeds this limit (USD).
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="flex items-center gap-3">
          <Label>Current cap:</Label>
          <span className="tabular-nums text-sm font-medium">
            {currentCap != null ? `$${currentCap.toFixed(2)}` : "—"}
          </span>
          <Separator orientation="vertical" className="h-5" />
          <Input
            type="number"
            step="0.50"
            min="0"
            placeholder="New cap (USD)"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            className="w-[140px]"
          />
          <Button
            size="sm"
            disabled={!value || isUpdating}
            onClick={() => {
              const cap = Number.parseFloat(value);
              if (!Number.isNaN(cap) && cap >= 0) {
                onUpdate(cap);
                setValue("");
              }
            }}
          >
            {isUpdating && <Loader2 className="mr-1.5 size-3.5 animate-spin" />}
            Update
          </Button>
        </div>
      </CardContent>
    </Card>
  );
};

// ---------------------------------------------------------------------------
// Storage Health
// ---------------------------------------------------------------------------

const StorageHealthSection = ({
  healthy,
  errorMessage,
}: {
  healthy: boolean;
  errorMessage: string | undefined;
}): React.ReactElement => (
  <Card>
    <CardHeader>
      <CardTitle className="text-base">Object Storage</CardTitle>
      <CardDescription>S3-compatible storage for artifacts and reports.</CardDescription>
    </CardHeader>
    <CardContent>
      <div className="flex items-center gap-2">
        <Database className="size-4 text-muted-foreground" />
        {healthy ? (
          <Badge variant="default" className="gap-1 text-[10px] uppercase">
            <CheckCircle2 className="size-3" />
            Healthy
          </Badge>
        ) : (
          <Badge variant="destructive" className="gap-1 text-[10px] uppercase">
            <XCircle className="size-3" />
            Unreachable
          </Badge>
        )}
        {errorMessage && <span className="text-xs text-muted-foreground">{errorMessage}</span>}
      </div>
    </CardContent>
  </Card>
);
