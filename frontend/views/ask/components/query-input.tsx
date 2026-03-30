import { ArrowUp, Square } from "lucide-react";
import { useCallback, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { ContextIndicator } from "@/views/ask/components/context-indicator";
import { ModelSelector } from "@/views/ask/components/model-selector";
import type { ContextUsage } from "@/views/ask/hooks/use-ask-question";

export const QueryInput = ({
  onSubmit,
  onCancel,
  isStreaming,
  disabled,
  selectedModel,
  onModelChange,
  contextUsage,
}: {
  onSubmit: (question: string) => void;
  onCancel: () => void;
  isStreaming: boolean;
  disabled?: boolean;
  selectedModel: string | undefined;
  onModelChange: (modelId: string | undefined) => void;
  contextUsage?: ContextUsage;
}): React.ReactElement => {
  const [value, setValue] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  const handleSubmit = useCallback(() => {
    const trimmed = value.trim();
    if (!trimmed || isStreaming) return;
    onSubmit(trimmed);
    setValue("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
  }, [value, isStreaming, onSubmit]);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSubmit();
      }
    },
    [handleSubmit],
  );

  const handleInput = useCallback(() => {
    const el = textareaRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  }, []);

  return (
    <div className="relative rounded-lg border bg-background shadow-sm focus-within:ring-1 focus-within:ring-ring">
      <textarea
        ref={textareaRef}
        value={value}
        onChange={(e) => {
          setValue(e.target.value);
          handleInput();
        }}
        onKeyDown={handleKeyDown}
        placeholder="Ask a question about your engineering data..."
        className="w-full resize-none bg-transparent px-4 pt-3 pb-12 text-sm outline-none placeholder:text-muted-foreground"
        rows={1}
        disabled={disabled}
      />
      <div className="absolute bottom-2 left-2 flex items-center gap-1">
        <ModelSelector value={selectedModel} onSelect={onModelChange} disabled={isStreaming} />
        {contextUsage && contextUsage.contextWindow > 0 && (
          <ContextIndicator contextUsage={contextUsage} onCompact={() => onSubmit("/compact")} />
        )}
      </div>
      <div className="absolute bottom-2 right-2">
        {isStreaming ? (
          <Button variant="destructive" size="icon" className="size-8" onClick={onCancel}>
            <Square className="size-3.5" />
          </Button>
        ) : (
          <Button
            size="icon"
            className="size-8"
            onClick={handleSubmit}
            disabled={!value.trim() || disabled}
          >
            <ArrowUp className="size-4" />
          </Button>
        )}
      </div>
    </div>
  );
};
