import { ArrowUp, Plus, Square } from "lucide-react";
import { useCallback, useRef, useState } from "react";

import { Button } from "@/components/ui/button";
import { ContextIndicator } from "@/views/ask/components/context-indicator";
import {
  FileAttachmentChips,
  type AttachedFileInfo,
} from "@/views/ask/components/file-attachment-chips";
import { ModelSelector } from "@/views/ask/components/model-selector";
import { PodStatusIndicator } from "@/views/ask/components/pod-status-indicator";
import type { ContextUsage } from "@/views/ask/hooks/use-ask-question";
import { cn } from "@ps/cn";

export const QueryInput = ({
  onSubmit,
  onCancel,
  isStreaming,
  disabled,
  selectedModel,
  onModelChange,
  contextUsage,
  containerStatus,
  podName,
  podIp,
  attachedFiles = [],
  onFilesAdded,
  onFileRemoved,
}: {
  onSubmit: (question: string) => void;
  onCancel: () => void;
  isStreaming: boolean;
  disabled?: boolean;
  selectedModel: string | undefined;
  onModelChange: (modelId: string | undefined) => void;
  contextUsage?: ContextUsage;
  containerStatus?: string;
  podName?: string;
  podIp?: string;
  attachedFiles?: AttachedFileInfo[];
  onFilesAdded?: (files: File[]) => void;
  onFileRemoved?: (index: number) => void;
}): React.ReactElement => {
  const [value, setValue] = useState("");
  const [dragActive, setDragActive] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const handleSubmit = useCallback(() => {
    const trimmed = value.trim();
    if ((!trimmed && attachedFiles.length === 0) || isStreaming) return;
    onSubmit(trimmed);
    setValue("");
    if (textareaRef.current) {
      textareaRef.current.style.height = "auto";
    }
  }, [value, isStreaming, onSubmit, attachedFiles.length]);

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

  const handleDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault();
      setDragActive(false);
      if (!onFilesAdded) return;
      const files = Array.from(e.dataTransfer.files);
      if (files.length > 0) onFilesAdded(files);
    },
    [onFilesAdded],
  );

  const handlePaste = useCallback(
    (e: React.ClipboardEvent) => {
      if (!onFilesAdded) return;
      const files = Array.from(e.clipboardData.files);
      if (files.length > 0) {
        e.preventDefault();
        onFilesAdded(files);
      }
    },
    [onFilesAdded],
  );

  const handleFileInput = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      if (!onFilesAdded) return;
      const files = Array.from(e.target.files ?? []);
      if (files.length > 0) onFilesAdded(files);
      e.target.value = "";
    },
    [onFilesAdded],
  );

  const hasContent = !!value.trim() || attachedFiles.length > 0;

  return (
    <div
      className={cn(
        "relative rounded-lg border bg-background shadow-sm focus-within:ring-1 focus-within:ring-ring",
        dragActive && "ring-2 ring-primary border-primary",
      )}
      onDragOver={(e) => {
        e.preventDefault();
        setDragActive(true);
      }}
      onDragLeave={() => setDragActive(false)}
      onDrop={handleDrop}
    >
      <textarea
        ref={textareaRef}
        value={value}
        onChange={(e) => {
          setValue(e.target.value);
          handleInput();
        }}
        onKeyDown={handleKeyDown}
        onPaste={handlePaste}
        placeholder={
          selectedModel?.startsWith("image:")
            ? "Describe an image..."
            : "Ask a question about your engineering data..."
        }
        className="w-full resize-none bg-transparent px-4 pt-3 pb-2 text-sm outline-none placeholder:text-muted-foreground"
        rows={1}
        disabled={disabled}
      />
      <FileAttachmentChips files={attachedFiles} onRemove={onFileRemoved} />
      <div className="flex items-center justify-between px-2 pb-2">
        <div className="flex items-center gap-1">
          {onFilesAdded && (
            <>
              <Button
                variant="ghost"
                size="icon"
                className="size-8"
                onClick={() => fileInputRef.current?.click()}
                disabled={isStreaming || disabled}
                title="Attach files"
              >
                <Plus className="size-4" />
              </Button>
              <input
                ref={fileInputRef}
                type="file"
                multiple
                onChange={handleFileInput}
                className="hidden"
              />
            </>
          )}
          <ModelSelector value={selectedModel} onSelect={onModelChange} disabled={isStreaming} />
        </div>
        <div className="flex items-center gap-2">
          {containerStatus && (
            <PodStatusIndicator
              containerStatus={containerStatus}
              podName={podName}
              podIp={podIp}
              isStreaming={isStreaming}
            />
          )}
          {contextUsage && contextUsage.contextWindow > 0 && (
            <ContextIndicator contextUsage={contextUsage} onCompact={() => onSubmit("/compact")} />
          )}
          {isStreaming ? (
            <Button variant="destructive" size="icon" className="size-8" onClick={onCancel}>
              <Square className="size-3.5" />
            </Button>
          ) : (
            <Button
              size="icon"
              className="size-8"
              onClick={handleSubmit}
              disabled={!hasContent || disabled}
            >
              <ArrowUp className="size-4" />
            </Button>
          )}
        </div>
      </div>
    </div>
  );
};
