import { useState } from "react";
import { ChevronsUpDown, ImageIcon } from "lucide-react";

import {
  Command,
  CommandEmpty,
  CommandGroup,
  CommandInput,
  CommandItem,
  CommandList,
} from "@/components/ui/command";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { AiProvider } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { AiModelInfo } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { aiProviderKey, aiProviderLabel } from "@/lib/proto-display";
import { useAiModels, useAiSettings } from "@/lib/hooks/use-ai-settings";

/** Google Gemini icon — the official gradient sparkle. */
const GeminiIcon = ({ className }: { className?: string }): React.ReactElement => (
  <svg viewBox="0 0 28 28" fill="none" className={className}>
    <path
      d="M14 28C14 21.75 9.53 16.57 3.72 15.42C2.48 15.18 1.23 15.07 0 15.07V12.93C1.23 12.93 2.48 12.82 3.72 12.58C9.53 11.43 14 6.25 14 0C14 6.25 18.47 11.43 24.28 12.58C25.52 12.82 26.77 12.93 28 12.93V15.07C26.77 15.07 25.52 15.18 24.28 15.42C18.47 16.57 14 21.75 14 28Z"
      fill="currentColor"
    />
  </svg>
);

/** Provider icon for the trigger button and dropdown items. */
const ProviderIcon = ({
  className = "size-4",
}: {
  provider?: AiProvider;
  className?: string;
}): React.ReactElement => <GeminiIcon className={className} />;

/** Format a context length like "1M" or "128K". */
const formatContext = (n: number): string => {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n % 1_000_000 === 0 ? 0 : 1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(n % 1_000 === 0 ? 0 : 1)}K`;
  return String(n);
};

/**
 * Build the override string the backend expects.
 * Chat models: "provider/model_id"
 * Image models: "image:provider/model_id"
 */
const toOverrideId = (model: AiModelInfo, isImage: boolean): string => {
  const base = `${aiProviderKey(model.provider)}/${model.id}`;
  return isImage ? `image:${base}` : base;
};

/** Check if a model is image-generation only (no completion capability). */
const isImageOnly = (model: AiModelInfo): boolean =>
  model.capabilities.includes("image_generation") && !model.capabilities.includes("completion");

/** Parse the selected value to determine if it's an image model. */
export const parseModelSelection = (
  value: string | undefined,
): { isImageModel: boolean; imageModel?: string; chatModelOverride?: string } => {
  if (!value) return { isImageModel: false };
  if (value.startsWith("image:")) {
    return { isImageModel: true, imageModel: value.slice(6) };
  }
  return { isImageModel: false, chatModelOverride: value };
};

export const ModelSelector = ({
  value,
  onSelect,
  disabled,
}: {
  value: string | undefined;
  onSelect: (modelId: string | undefined) => void;
  disabled?: boolean;
}): React.ReactElement => {
  const [open, setOpen] = useState(false);

  // Fetch all models (no capability filter) — we group them client-side.
  const { data: allModelsResponse } = useAiModels(undefined, undefined);
  const { data: settings } = useAiSettings();

  const allModels = allModelsResponse?.models ?? [];

  // Split into chat models (have tool_use) and image-only models.
  const chatModels = allModels.filter(
    (m) => m.capabilities.includes("tool_use") && !isImageOnly(m),
  );
  const imageModels = allModels.filter((m) => isImageOnly(m));

  // Find the selected model.
  const isImage = value?.startsWith("image:");
  const lookupId = isImage ? value?.slice(6) : value;
  const selectedPool = isImage ? imageModels : chatModels;
  const selected = lookupId
    ? selectedPool.find((m) => toOverrideId(m, false) === lookupId)
    : undefined;

  // Resolve the admin-configured default model.
  const defaultModelId = settings?.agentic?.model;
  const defaultProvider = settings?.agentic?.provider;
  const defaultModel = defaultModelId ? chatModels.find((m) => m.id === defaultModelId) : undefined;
  const defaultLabel = defaultModel?.displayName ?? defaultModelId ?? "Default model";

  const activeProvider = selected?.provider ?? defaultModel?.provider ?? defaultProvider;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger
        render={
          <button
            type="button"
            disabled={disabled}
            className="inline-flex h-7 items-center gap-1 rounded-md bg-transparent px-1 text-sm text-muted-foreground transition-colors hover:text-foreground disabled:cursor-not-allowed disabled:opacity-50"
          />
        }
      >
        {isImage ? (
          <ImageIcon className="size-4 shrink-0 text-muted-foreground" />
        ) : (
          activeProvider != null && (
            <ProviderIcon
              provider={activeProvider}
              className="size-4 shrink-0 text-muted-foreground"
            />
          )
        )}
        <span className="min-w-0 truncate">{selected ? selected.displayName : defaultLabel}</span>
        <ChevronsUpDown className="size-3.5 shrink-0 text-muted-foreground" />
      </PopoverTrigger>
      <PopoverContent className="w-[360px] p-0" align="start">
        <Command shouldFilter>
          <CommandInput placeholder="Search models..." />
          <CommandList>
            <CommandEmpty>
              {allModels.length === 0
                ? "No models available. Check AI provider settings."
                : "No matching models."}
            </CommandEmpty>

            {/* Default model */}
            <CommandGroup>
              <CommandItem
                value="__default__"
                data-checked={!value ? "true" : undefined}
                onSelect={() => {
                  onSelect(undefined);
                  setOpen(false);
                }}
              >
                {defaultProvider != null && (
                  <ProviderIcon
                    provider={defaultProvider}
                    className="size-4 shrink-0 text-muted-foreground"
                  />
                )}
                <span className="text-sm">{defaultLabel} (default)</span>
              </CommandItem>
            </CommandGroup>

            {/* Chat models */}
            {chatModels.length > 0 && (
              <CommandGroup heading="Chat Models">
                {chatModels.map((m) => {
                  const overrideId = toOverrideId(m, false);
                  return (
                    <CommandItem
                      key={overrideId}
                      value={`${m.id} ${m.displayName}`}
                      data-checked={overrideId === value ? "true" : undefined}
                      onSelect={() => {
                        onSelect(overrideId);
                        setOpen(false);
                      }}
                    >
                      <ProviderIcon
                        provider={m.provider}
                        className="size-4 shrink-0 text-muted-foreground"
                      />
                      <span className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate text-sm">{m.displayName || m.id}</span>
                        {m.contextLength > 0 && (
                          <span className="text-xs text-muted-foreground">
                            {aiProviderLabel(m.provider)} · {formatContext(m.contextLength)} context
                          </span>
                        )}
                      </span>
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            )}

            {/* Image generation models */}
            {imageModels.length > 0 && (
              <CommandGroup heading="Image Generation">
                {imageModels.map((m) => {
                  const overrideId = toOverrideId(m, true);
                  return (
                    <CommandItem
                      key={overrideId}
                      value={`${m.id} ${m.displayName} image`}
                      data-checked={overrideId === value ? "true" : undefined}
                      onSelect={() => {
                        onSelect(overrideId);
                        setOpen(false);
                      }}
                    >
                      <ImageIcon className="size-4 shrink-0 text-muted-foreground" />
                      <span className="flex min-w-0 flex-1 flex-col">
                        <span className="truncate text-sm">{m.displayName || m.id}</span>
                        <span className="text-xs text-muted-foreground">
                          {aiProviderLabel(m.provider)}
                        </span>
                      </span>
                    </CommandItem>
                  );
                })}
              </CommandGroup>
            )}
          </CommandList>
        </Command>
      </PopoverContent>
    </Popover>
  );
};
