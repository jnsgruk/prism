import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import {
  type ArtifactDisplay,
  type FileNode,
  canPreview,
  formatSize,
  getFileIcon,
} from "@/views/ask/hooks/use-file-tree";
import {
  ChevronDown,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  Download,
  Eye,
  Folder,
  FolderOpen,
  Search,
} from "lucide-react";
import { useCallback, useMemo, useRef, useState } from "react";

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/** Collect all directory paths in the tree. */
const collectDirPaths = (nodes: FileNode[]): Set<string> => {
  const paths = new Set<string>();
  const walk = (list: FileNode[]): void => {
    for (const n of list) {
      if (n.isDirectory) {
        paths.add(n.path);
        walk(n.children);
      }
    }
  };
  walk(nodes);
  return paths;
};

/** Return paths of nodes (and their ancestors) that match a filter string. */
const findMatchingPaths = (nodes: FileNode[], filter: string): Set<string> => {
  const paths = new Set<string>();
  const lower = filter.toLowerCase();

  const walk = (list: FileNode[]): boolean => {
    let anyMatch = false;
    for (const n of list) {
      const childMatch = n.isDirectory ? walk(n.children) : false;
      const selfMatch = n.name.toLowerCase().includes(lower);
      if (selfMatch || childMatch) {
        paths.add(n.path);
        anyMatch = true;
      }
    }
    return anyMatch;
  };
  walk(nodes);
  return paths;
};

// ---------------------------------------------------------------------------
// Tree node
// ---------------------------------------------------------------------------

const FileTreeNode = ({
  node,
  depth,
  expandedPaths,
  toggleExpanded,
  selectedPath,
  matchingPaths,
  onPreview,
  onDownload,
}: {
  node: FileNode;
  depth: number;
  expandedPaths: Set<string>;
  toggleExpanded: (path: string) => void;
  selectedPath: string | null;
  matchingPaths: Set<string> | null;
  onPreview: (artifact: ArtifactDisplay) => void;
  onDownload: (artifact: ArtifactDisplay) => void;
}): React.ReactElement | null => {
  if (matchingPaths && !matchingPaths.has(node.path)) return null;

  const isExpanded = expandedPaths.has(node.path);
  const isSelected = selectedPath === node.path;
  let Icon: typeof Folder;
  if (node.isDirectory) {
    Icon = isExpanded ? FolderOpen : Folder;
  } else {
    Icon = getFileIcon(node.name, node.artifact?.contentType);
  }

  return (
    <>
      <button
        type="button"
        className={`group flex w-full items-center gap-1.5 px-2 py-1 text-left text-sm transition-colors hover:bg-muted/50 ${
          isSelected ? "bg-muted" : ""
        }`}
        style={{ paddingLeft: `${depth * 12 + 8}px` }}
        onClick={() => {
          if (node.isDirectory) {
            toggleExpanded(node.path);
          } else if (node.artifact && canPreview(node.artifact.contentType)) {
            onPreview(node.artifact);
          }
        }}
      >
        {node.isDirectory ? (
          <button
            type="button"
            className="shrink-0 rounded p-0.5 hover:bg-muted"
            onClick={(e) => {
              e.stopPropagation();
              toggleExpanded(node.path);
            }}
          >
            {isExpanded ? <ChevronDown className="size-3" /> : <ChevronRight className="size-3" />}
          </button>
        ) : (
          <span className="w-4" />
        )}

        <Icon className="size-3.5 shrink-0 text-muted-foreground" />
        <span className="min-w-0 flex-1 truncate">{node.name}</span>

        {!node.isDirectory && node.artifact && (
          <>
            <span className="shrink-0 text-[10px] text-muted-foreground opacity-0 group-hover:opacity-100">
              {formatSize(node.artifact.sizeBytes)}
            </span>

            <span
              className="flex shrink-0 items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100"
              onClick={(e) => e.stopPropagation()}
            >
              {canPreview(node.artifact.contentType) && (
                <Tooltip>
                  <TooltipTrigger render={<Button variant="ghost" size="icon" className="size-5" />}>
                    <Eye className="size-3" onClick={() => node.artifact && onPreview(node.artifact)} />
                  </TooltipTrigger>
                  <TooltipContent>Preview</TooltipContent>
                </Tooltip>
              )}
              <Tooltip>
                <TooltipTrigger render={<Button variant="ghost" size="icon" className="size-5" />}>
                  <Download className="size-3" onClick={() => node.artifact && onDownload(node.artifact)} />
                </TooltipTrigger>
                <TooltipContent>Download</TooltipContent>
              </Tooltip>
            </span>
          </>
        )}
      </button>

      {node.isDirectory &&
        isExpanded &&
        node.children.map((child) => (
          <FileTreeNode
            key={child.path}
            node={child}
            depth={depth + 1}
            expandedPaths={expandedPaths}
            toggleExpanded={toggleExpanded}
            selectedPath={selectedPath}
            matchingPaths={matchingPaths}
            onPreview={onPreview}
            onDownload={onDownload}
          />
        ))}
    </>
  );
};

// ---------------------------------------------------------------------------
// Main tree component
// ---------------------------------------------------------------------------

export const WorkspaceTree = ({
  roots,
  selectedPath,
  onPreview,
  onDownload,
}: {
  roots: FileNode[];
  selectedPath: string | null;
  onPreview: (artifact: ArtifactDisplay) => void;
  onDownload: (artifact: ArtifactDisplay) => void;
}): React.ReactElement => {
  const [filter, setFilter] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  const debounceRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const [debouncedFilter, setDebouncedFilter] = useState("");

  const handleFilterChange = useCallback((value: string) => {
    setFilter(value);
    clearTimeout(debounceRef.current);
    debounceRef.current = setTimeout(() => setDebouncedFilter(value), 300);
  }, []);

  const allDirPaths = useMemo(() => collectDirPaths(roots), [roots]);

  // Start with only the top-level workspace/ folder expanded.
  // State persists while the component is mounted (sidebar open/close is CSS-only).
  const [expandedPaths, setExpandedPaths] = useState<Set<string>>(() => new Set([""]));

  const toggleExpanded = useCallback((path: string) => {
    setExpandedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const expandAll = useCallback(() => setExpandedPaths(allDirPaths), [allDirPaths]);
  const collapseAll = useCallback(() => setExpandedPaths(new Set()), []);

  const matchingPaths = useMemo(
    () => (debouncedFilter.trim() ? findMatchingPaths(roots, debouncedFilter.trim()) : null),
    [roots, debouncedFilter],
  );

  return (
    <div className="flex flex-col">
      <div className="sticky top-0 z-10 flex items-center gap-1.5 border-b bg-background px-2 py-1.5">
        <div className="relative flex-1">
          <Search className="absolute top-1/2 left-2 size-3.5 -translate-y-1/2 text-muted-foreground" />
          <Input
            ref={inputRef}
            placeholder="Filter files..."
            value={filter}
            onChange={(e) => handleFilterChange(e.target.value)}
            className="h-7 pl-7 text-xs"
          />
        </div>
        <Button variant="ghost" size="icon" className="size-6" title="Expand all" onClick={expandAll}>
          <ChevronsUpDown className="size-3" />
        </Button>
        <Button variant="ghost" size="icon" className="size-6" title="Collapse all" onClick={collapseAll}>
          <ChevronsDownUp className="size-3" />
        </Button>
      </div>

      <div className="overflow-y-auto">
        {roots.map((node) => (
          <FileTreeNode
            key={node.path}
            node={node}
            depth={0}
            expandedPaths={expandedPaths}
            toggleExpanded={toggleExpanded}
            selectedPath={selectedPath}
            matchingPaths={matchingPaths}
            onPreview={onPreview}
            onDownload={onDownload}
          />
        ))}
      </div>
    </div>
  );
};
