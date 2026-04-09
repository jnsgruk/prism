import { File, User, Users } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { Person, Team } from "@ps/api/gen/canonical/prism/v1/org_pb";
import { getFileIcon, formatSize } from "@/views/ask/hooks/use-file-tree";
import type { WorkspaceFileDisplay } from "@/views/ask/hooks/use-file-tree";
import type { MentionType } from "@/views/ask/hooks/use-mention-picker";
import { cn } from "@ps/cn";

/** Simple fuzzy-ish match: all query characters appear in order in the target. */
const matchesQuery = (target: string, query: string): boolean => {
  if (!query) return true;
  const lower = target.toLowerCase();
  const q = query.toLowerCase();
  let qi = 0;
  for (let i = 0; i < lower.length && qi < q.length; i++) {
    if (lower[i] === q[qi]) qi++;
  }
  return qi === q.length;
};

const MAX_PER_CATEGORY = 20;

type CategoryItem = {
  id: string;
  name: string;
  type: MentionType;
  subtitle?: string;
  icon: React.ComponentType<{ className?: string }>;
};

export type NavigateHandle = {
  moveUp: () => void;
  moveDown: () => void;
  selectCurrent: () => void;
};

export const MentionPopover = ({
  query,
  files,
  people,
  teams,
  onSelect,
  onClose,
  onNavigate,
}: {
  query: string;
  files: WorkspaceFileDisplay[];
  people: Person[];
  teams: Team[];
  onSelect: (id: string, name: string, type: MentionType) => void;
  onClose: () => void;
  onNavigate?: React.RefObject<NavigateHandle | null>;
}): React.ReactElement => {
  const [selectedIndex, setSelectedIndex] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  // Build categorised, filtered items
  const { items, categories } = useMemo(() => {
    const allItems: CategoryItem[] = [];
    const cats: { label: string; startIndex: number; count: number }[] = [];

    // People
    const filteredPeople = people
      .filter((p) => p.active && matchesQuery(p.name, query))
      .slice(0, MAX_PER_CATEGORY);
    if (filteredPeople.length > 0) {
      cats.push({ label: "People", startIndex: allItems.length, count: filteredPeople.length });
      for (const p of filteredPeople) {
        allItems.push({
          id: p.id,
          name: p.name,
          type: "person",
          subtitle: p.teamName ?? undefined,
          icon: User,
        });
      }
    }

    // Teams
    const filteredTeams = teams
      .filter((t) => matchesQuery(t.name, query))
      .slice(0, MAX_PER_CATEGORY);
    if (filteredTeams.length > 0) {
      cats.push({ label: "Teams", startIndex: allItems.length, count: filteredTeams.length });
      for (const t of filteredTeams) {
        allItems.push({
          id: t.id,
          name: t.name,
          type: "team",
          subtitle: `${t.memberCount} member${t.memberCount === 1 ? "" : "s"}`,
          icon: Users,
        });
      }
    }

    // Files (only when available)
    const filteredFiles = files
      .filter((f) => !f.isDirectory && matchesQuery(f.path, query))
      .slice(0, MAX_PER_CATEGORY);
    if (filteredFiles.length > 0) {
      cats.push({ label: "Files", startIndex: allItems.length, count: filteredFiles.length });
      for (const f of filteredFiles) {
        const name = f.path.split("/").pop() ?? f.path;
        allItems.push({
          id: f.path,
          name,
          type: "file",
          subtitle: formatSize(f.sizeBytes),
          icon: getFileIcon(name, f.contentType) ?? File,
        });
      }
    }

    return { items: allItems, categories: cats };
  }, [people, teams, files, query]);

  // Reset selection when filtered list changes
  useEffect(() => {
    setSelectedIndex(0);
  }, [items.length, query]);

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
    setSelectedIndex((prev) => (prev <= 0 ? items.length - 1 : prev - 1));
  }, [items.length]);

  const moveDown = useCallback(() => {
    setSelectedIndex((prev) => (prev >= items.length - 1 ? 0 : prev + 1));
  }, [items.length]);

  const selectCurrent = useCallback(() => {
    const item = items[selectedIndex];
    if (item) {
      onSelect(item.id, item.name, item.type);
    }
  }, [items, selectedIndex, onSelect]);

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

  // Determine which category header appears before each item index
  const categoryStartIndices = new Set(categories.map((c) => c.startIndex));

  return (
    <div
      ref={containerRef}
      className="absolute bottom-full left-0 z-50 mb-1 w-80 rounded-lg border bg-popover p-1 shadow-md"
    >
      <div ref={listRef} className="max-h-60 overflow-y-auto">
        {items.length === 0 && (
          <p className="py-4 text-center text-sm text-muted-foreground">No results</p>
        )}
        {items.map((item, index) => {
          const cat = categoryStartIndices.has(index)
            ? categories.find((c) => c.startIndex === index)
            : undefined;
          const isSelected = index === selectedIndex;
          const Icon = item.icon;
          return (
            <div key={`${item.type}-${item.id}`}>
              {cat && (
                <div className="px-2 pt-2 pb-1 text-[10px] font-medium uppercase tracking-wider text-muted-foreground">
                  {cat.label}
                </div>
              )}
              <button
                type="button"
                data-selected={isSelected || undefined}
                className={cn(
                  "flex w-full items-center gap-2 rounded-sm px-2 py-1.5 text-left text-sm outline-none",
                  isSelected ? "bg-accent text-accent-foreground" : "text-popover-foreground",
                )}
                onMouseEnter={() => setSelectedIndex(index)}
                onMouseDown={(e) => {
                  e.preventDefault();
                  onSelect(item.id, item.name, item.type);
                }}
              >
                <Icon className="size-4 shrink-0 text-muted-foreground" />
                <span className="min-w-0 flex-1 truncate">
                  {item.type === "file" ? item.id : item.name}
                </span>
                {item.subtitle && (
                  <span className="shrink-0 text-xs text-muted-foreground">{item.subtitle}</span>
                )}
              </button>
            </div>
          );
        })}
      </div>
    </div>
  );
};
