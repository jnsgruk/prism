import { Button } from "@/components/ui/button";
import { useListWorkspaceFiles } from "@/lib/hooks/use-conversations";
import { ContextIndicator } from "@/views/ask/components/context-indicator";
import { FileAttachmentChips, type AttachedFileInfo } from "@/views/ask/components/file-attachment-chips";
import { MentionPopover, type NavigateHandle } from "@/views/ask/components/mention-popover";
import { ModelSelector } from "@/views/ask/components/model-selector";
import { PodStatusIndicator } from "@/views/ask/components/pod-status-indicator";
import type { ContextUsage } from "@/views/ask/hooks/use-ask-question";
import type { WorkspaceFileDisplay } from "@/views/ask/hooks/use-file-tree";
import { useMentionPicker, extractContent, type MentionItem } from "@/views/ask/hooks/use-mention-picker";
import { useListPeople, useListTeams } from "@/views/teams/hooks/use-teams";
import { ArrowUp, Plus, Square } from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";

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
  conversationId,
}: {
  onSubmit: (question: string, mentions?: MentionItem[]) => void;
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
  conversationId?: string;
}): React.ReactElement => {
  const [dragActive, setDragActive] = useState(false);
  const [hasContent, setHasContent] = useState(false);
  const editorRef = useRef<HTMLDivElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const { mentionQuery, mentionActive, detectMention, insertPill, closeMention } = useMentionPicker();

  // Workspace files for the @ mention picker (only when conversation exists)
  const { data: workspaceData } = useListWorkspaceFiles(conversationId ?? "");
  const workspaceFiles: WorkspaceFileDisplay[] = useMemo(
    () =>
      (workspaceData?.files ?? []).map((f) => ({
        path: f.path,
        sizeBytes: Number(f.sizeBytes),
        isDirectory: f.isDirectory,
        contentType: f.contentType,
      })),
    [workspaceData],
  );

  // People and teams for @ mention picker (always available)
  const { data: people = [] } = useListPeople();
  const { data: teams = [] } = useListTeams();

  const getEditorContent = useCallback(() => {
    const el = editorRef.current;
    if (!el) return { text: "", mentions: [] as MentionItem[] };
    return extractContent(el);
  }, []);

  const clearEditor = useCallback(() => {
    const el = editorRef.current;
    if (el) {
      el.innerHTML = "";
      setHasContent(false);
    }
  }, []);

  const handleSubmit = useCallback(() => {
    if (isStreaming) return;
    const { text, mentions } = getEditorContent();
    if (!text && attachedFiles.length === 0 && mentions.length === 0) return;
    onSubmit(text, mentions.length > 0 ? mentions : undefined);
    clearEditor();
  }, [isStreaming, onSubmit, attachedFiles.length, getEditorContent, clearEditor]);

  const handleMentionSelect = useCallback(
    (id: string, name: string, type: "file" | "person" | "team") => {
      const el = editorRef.current;
      if (!el) return;
      insertPill(id, name, type, el);
      el.focus();
    },
    [insertPill],
  );

  // Imperative handle for popover keyboard navigation
  const popoverNav = useRef<NavigateHandle | null>(null);

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent) => {
      // When mention picker is active, handle navigation
      if (mentionActive) {
        if (e.key === "Escape") {
          e.preventDefault();
          closeMention();
          return;
        }
        if (e.key === "ArrowUp") {
          e.preventDefault();
          popoverNav.current?.moveUp();
          return;
        }
        if (e.key === "ArrowDown") {
          e.preventDefault();
          popoverNav.current?.moveDown();
          return;
        }
        if (e.key === "Enter" && !e.shiftKey) {
          e.preventDefault();
          popoverNav.current?.selectCurrent();
          return;
        }
      }

      if (e.key === "Enter" && !e.shiftKey) {
        e.preventDefault();
        handleSubmit();
      }
    },
    [handleSubmit, mentionActive, closeMention],
  );

  const handleInput = useCallback(() => {
    // Update hasContent state
    const el = editorRef.current;
    if (!el) return;
    const { text, mentions } = extractContent(el);
    setHasContent(!!text || mentions.length > 0);

    // Detect @ mentions — people/teams always available, files need a conversation
    detectMention();
  }, [detectMention]);

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
      // Handle file paste
      if (onFilesAdded) {
        const files = Array.from(e.clipboardData.files);
        if (files.length > 0) {
          e.preventDefault();
          onFilesAdded(files);
          return;
        }
      }
      // For text paste, insert as plain text to avoid formatting
      e.preventDefault();
      const text = e.clipboardData.getData("text/plain");
      if (text) {
        const selection = window.getSelection();
        if (selection && selection.rangeCount > 0) {
          const range = selection.getRangeAt(0);
          range.deleteContents();
          range.insertNode(document.createTextNode(text));
          range.collapse(false);
        }
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

  const enableSubmit = hasContent || attachedFiles.length > 0;

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
      {/* @ mention popover — positioned above the input */}
      {mentionActive && (
        <MentionPopover
          query={mentionQuery ?? ""}
          files={conversationId ? workspaceFiles : []}
          people={people}
          teams={teams}
          onSelect={handleMentionSelect}
          onClose={closeMention}
          onNavigate={popoverNav}
        />
      )}

      {/* contentEditable input with inline pill support */}
      <div
        ref={editorRef}
        contentEditable={!disabled}
        role="textbox"
        aria-multiline
        aria-placeholder={
          selectedModel?.startsWith("image:") ? "Describe an image..." : "Ask a question about your engineering data..."
        }
        onInput={handleInput}
        onKeyDown={handleKeyDown}
        onPaste={handlePaste}
        data-placeholder={
          selectedModel?.startsWith("image:") ? "Describe an image..." : "Ask a question about your engineering data..."
        }
        className="min-h-[36px] max-h-[200px] w-full overflow-y-auto bg-transparent px-4 pt-3 pb-2 text-sm outline-none empty:before:text-muted-foreground empty:before:content-[attr(data-placeholder)]"
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
              <input ref={fileInputRef} type="file" multiple onChange={handleFileInput} className="hidden" />
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
            <Button size="icon" className="size-8" onClick={handleSubmit} disabled={!enableSubmit || disabled}>
              <ArrowUp className="size-4" />
            </Button>
          )}
        </div>
      </div>
    </div>
  );
};
