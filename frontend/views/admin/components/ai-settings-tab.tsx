import { useState } from "react";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
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
import { CheckCircle2, Database, Eye, EyeOff, Loader2, XCircle } from "lucide-react";
import { toast } from "sonner";

import { AiCostSection } from "@/views/admin/components/ai-cost-tab";
import {
  useAiSettings,
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
  { key: "enrichment", label: "Enrichment", description: "High-volume metadata tagging" },
  { key: "insights", label: "Insights", description: "Deep reasoning for reports" },
  { key: "agentic", label: "Agentic", description: "Tool-use for natural language queries" },
  { key: "embeddings", label: "Embeddings", description: "Vector generation for similarity" },
] as const;

export const AiSettingsTab = (): React.ReactElement => {
  const { data: settings, isLoading } = useAiSettings();
  const updateSettings = useUpdateAiSettings();
  const setSecret = useSetProviderSecret();
  const testProvider = useTestProvider();
  const { data: storageHealth } = useStorageHealth();

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
        isSettingSecret={setSecret.isPending}
        isTesting={testProvider.isPending}
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
  isSettingSecret,
  isTesting,
}: {
  secretStatus: Record<string, boolean>;
  onSetSecret: (provider: string, value: string) => void;
  onTestProvider: (provider: string) => void;
  isSettingSecret: boolean;
  isTesting: boolean;
}): React.ReactElement => (
  <Card>
    <CardHeader>
      <CardTitle className="text-base">Provider Credentials</CardTitle>
      <CardDescription>API keys are encrypted at rest. Values are never displayed.</CardDescription>
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
                <Input
                  className="w-[220px]"
                  value={config.model}
                  placeholder="Model name"
                  onChange={(e) => {
                    onUpdate({ [task.key]: { provider: config.provider, model: e.target.value } });
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
