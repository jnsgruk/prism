import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { getFileIcon, formatSize } from "@/views/ask/hooks/use-file-tree";
import type { WorkspaceFileDisplay } from "@/views/ask/hooks/use-file-tree";
import { cn } from "@ps/cn";

/** Simple fuzzy-ish match: all query characters appear in order in the path. */
const matchesQuery = (path: string, query: string): boolean => {
  if (!query) return true;
  const lower = path.toLowerCase();
  const q = query.toLowerCase();
  let qi = 0;
  for (let i = 0; i < lower.length && qi < q.length; i++) {
    if (lower[i] === q[qi]) qi++;
  }
  return qi === q.length;
};

export const FileMentionPopover = ({
  query,
  files,
  onSelect,
  onClose,
  onNavigate,
}: {
  query: string;
  files: WorkspaceFileDisplay[];
  onSelect: (path: string, name: string) => void;
  onClose: () => void;
  /** Expose imperative navigation to the parent (query-input keydown handler). */
  onNavigate?: React.RefObject<{
    moveUp: () => void;
    moveDown: () => void;
    selectCurrent: () => void;
  } | null>;
}): React.ReactElement => {
  const [selectedIndex, setSelectedIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  // Only show non-directory files, filtered by query
  const filtered = useMemo(
    () => files.filter((f) => !f.isDirectory && matchesQuery(f.path, query)),
    [files, query],
  );

  // Reset selection when filtered list changes
  useEffect(() => {
    setSelectedIndex(0);
  }, [filtered.length, query]);

  // Scroll selected item into view
  useEffect(() => {
    const list = listRef.current;
    if (!list) return;
    const selected = list.querySelector("[data-selected=true]");
    if (selected) {
      selected.scrollIntoView({ block: "nearest" });
    }
  }, [selectedIndex]);

  const moveUp = useCallback(() => {
    setSelectedIndex((prev) => (prev <= 0 ? filtered.length - 1 : prev - 1));
  }, [filtered.length]);

  const moveDown = useCallback(() => {
    setSelectedIndex((prev) => (prev >= filtered.length - 1 ? 0 : prev + 1));
  }, [filtered.length]);

  const selectCurrent = useCallback(() => {
    const file = filtered[selectedIndex];
    if (file) {
      const name = file.path.split("/").pop() ?? file.path;
      onSelect(file.path, name);
    }
  }, [filtered, selectedIndex, onSelect]);

  // Expose imperative handle to parent
  useEffect(() => {
    if (onNavigate) {
      onNavigate.current = { moveUp, moveDown, selectCurrent };
    }
    return (): void => {
      if (onNavigate) onNavigate.current = null;
    };
  }, [onNavigate, moveUp, moveDown, selectCurrent]);

  // Close on click outside
  const containerRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const handler = (e: MouseEvent): void => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        onClose();
      }
    };
    document.addEventListener("mousedown", handler);
    return (): void => document.removeEventListener("mousedown", handler);
  }, [onClose]);

  return (
    <div
      ref={containerRef}
      className="absolute bottom-full left-0 z-50 mb-1 w-80 rounded-lg border bg-popover p-1 shadow-md"
    >
      <div ref={listRef} className="max-h-48 overflow-y-auto">
        {filtered.length === 0 && (
          <p className="py-4 text-center text-sm text-muted-foreground">No matching files</p>
        )}
        {filtered.map((file, index) => {
          const name = file.path.split("/").pop() ?? file.path;
          const Icon = getFileIcon(name, file.contentType);
          const isSelected = index === selectedIndex;
          return (
            <button
              type="button"
              key={file.path}
              data-selected={isSelected || undefined}
              className={cn(
                "flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none",
                isSelected ? "bg-accent text-accent-foreground" : "text-popover-foreground",
              )}
              onMouseEnter={() => setSelectedIndex(index)}
              onMouseDown={(e) => {
                e.preventDefault();
                onSelect(file.path, name);
              }}
            >
              <Icon className="size-4 shrink-0 text-muted-foreground" />
              <span className="min-w-0 flex-1 truncate">{file.path}</span>
              <span className="shrink-0 text-xs text-muted-foreground">
                {formatSize(file.sizeBytes)}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
};
