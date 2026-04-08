import { useMemo } from "react";
import type { LucideIcon } from "lucide-react";
import {
  File,
  FileArchive,
  FileCode,
  FileSpreadsheet,
  FileText,
  ImageIcon,
  FileVideo,
  FileAudio,
} from "lucide-react";

/** Minimal file shape needed for the file tree. */
export type ArtifactDisplay = {
  id: string;
  displayName: string;
  contentType?: string;
  sizeBytes: bigint | number;
};

export type FileNode = {
  /** Segment name, e.g. "report.pdf" or "charts" */
  name: string;
  /** Full relative path from conversation root */
  path: string;
  isDirectory: boolean;
  children: FileNode[];
  /** Present only for leaf file nodes */
  artifact?: ArtifactDisplay;
};

// ---------------------------------------------------------------------------
// Icon mapping
// ---------------------------------------------------------------------------

const EXTENSION_LANG_MAP: Record<string, string> = {
  ".ts": "typescript",
  ".tsx": "typescript",
  ".js": "javascript",
  ".jsx": "javascript",
  ".mjs": "javascript",
  ".cjs": "javascript",
  ".py": "python",
  ".rs": "rust",
  ".go": "go",
  ".sql": "sql",
  ".sh": "bash",
  ".bash": "bash",
  ".zsh": "bash",
  ".fish": "bash",
  ".yaml": "yaml",
  ".yml": "yaml",
  ".toml": "toml",
  ".md": "markdown",
  ".mdx": "markdown",
  ".html": "html",
  ".htm": "html",
  ".css": "css",
  ".scss": "css",
  ".json": "json",
  ".jsonc": "json",
  ".xml": "xml",
  ".svg": "xml",
  ".java": "java",
  ".kt": "kotlin",
  ".rb": "ruby",
  ".php": "php",
  ".c": "c",
  ".h": "c",
  ".cpp": "cpp",
  ".hpp": "cpp",
  ".cs": "csharp",
  ".swift": "swift",
  ".r": "r",
  ".R": "r",
  ".lua": "lua",
  ".dockerfile": "dockerfile",
  ".proto": "protobuf",
  ".graphql": "graphql",
  ".gql": "graphql",
  ".nix": "nix",
};

const CONTENT_TYPE_LANG_MAP: Record<string, string> = {
  "application/json": "json",
  "application/xml": "xml",
  "text/xml": "xml",
  "text/html": "html",
  "text/css": "css",
  "text/javascript": "javascript",
  "application/javascript": "javascript",
  "text/x-python": "python",
  "text/x-rust": "rust",
  "text/x-go": "go",
  "text/yaml": "yaml",
  "text/x-yaml": "yaml",
  "application/x-yaml": "yaml",
  "text/markdown": "markdown",
  "text/x-toml": "toml",
  "text/csv": "csv",
  "text/x-sql": "sql",
  "text/x-shellscript": "bash",
  "text/x-c": "c",
  "text/x-c++src": "cpp",
  "text/x-java-source": "java",
  "text/x-ruby": "ruby",
  "text/x-php": "php",
  "application/typescript": "typescript",
};

/** Resolve the shiki language ID for a file, or undefined for plain text. */
export const resolveLanguage = (displayName: string, contentType?: string): string | undefined => {
  // Try content type first
  if (contentType && CONTENT_TYPE_LANG_MAP[contentType]) {
    return CONTENT_TYPE_LANG_MAP[contentType];
  }
  // Fall back to file extension
  const dot = displayName.lastIndexOf(".");
  if (dot >= 0) {
    const ext = displayName.slice(dot).toLowerCase();
    if (EXTENSION_LANG_MAP[ext]) return EXTENSION_LANG_MAP[ext];
  }
  return undefined;
};

/** Return the appropriate Lucide icon for a file based on content type / extension. */
export const getFileIcon = (displayName: string, contentType?: string): LucideIcon => {
  if (contentType) {
    if (contentType.startsWith("image/")) return ImageIcon;
    if (contentType.startsWith("video/")) return FileVideo;
    if (contentType.startsWith("audio/")) return FileAudio;
    if (contentType === "application/pdf") return FileText;
    if (
      contentType === "application/zip" ||
      contentType === "application/gzip" ||
      contentType === "application/x-tar" ||
      contentType === "application/x-7z-compressed"
    )
      return FileArchive;
    if (contentType === "text/csv" || contentType === "application/vnd.ms-excel")
      return FileSpreadsheet;
    if (
      contentType.startsWith("text/") ||
      contentType === "application/json" ||
      contentType === "application/xml" ||
      contentType === "application/javascript" ||
      contentType === "application/typescript"
    )
      return FileCode;
  }
  // Fall back to extension
  const dot = displayName.lastIndexOf(".");
  if (dot >= 0) {
    const ext = displayName.slice(dot).toLowerCase();
    if (EXTENSION_LANG_MAP[ext]) return FileCode;
    if ([".png", ".jpg", ".jpeg", ".gif", ".webp", ".svg", ".bmp", ".ico"].includes(ext))
      return ImageIcon;
    if ([".mp4", ".webm", ".mov", ".avi"].includes(ext)) return FileVideo;
    if ([".mp3", ".wav", ".ogg", ".flac"].includes(ext)) return FileAudio;
    if ([".zip", ".gz", ".tar", ".7z", ".rar"].includes(ext)) return FileArchive;
    if ([".pdf"].includes(ext)) return FileText;
    if ([".csv", ".xls", ".xlsx"].includes(ext)) return FileSpreadsheet;
  }
  return File;
};

// ---------------------------------------------------------------------------
// Tree building
// ---------------------------------------------------------------------------

type MutableNode = {
  name: string;
  path: string;
  children: Map<string, MutableNode>;
  artifact?: ArtifactDisplay;
};

const sortNodes = (nodes: FileNode[]): FileNode[] =>
  nodes.sort((a, b) => {
    if (a.isDirectory !== b.isDirectory) return a.isDirectory ? -1 : 1;
    return a.name.localeCompare(b.name);
  });

const toFileNode = (node: MutableNode): FileNode => {
  const children = [...node.children.values()].map(toFileNode);
  const isDirectory = children.length > 0;
  return {
    name: node.name,
    path: node.path,
    isDirectory,
    children: sortNodes(children),
    artifact: node.artifact,
  };
};

/** Workspace file info from the ListWorkspaceFiles RPC. */
export type WorkspaceFileDisplay = {
  path: string;
  sizeBytes: number | bigint;
  isDirectory: boolean;
  contentType?: string;
};

const buildWorkspaceTree = (files: WorkspaceFileDisplay[]): FileNode[] => {
  const root: MutableNode = { name: "", path: "", children: new Map() };

  for (const file of files) {
    const segments = file.path.split("/").filter(Boolean);

    let current = root;
    for (let i = 0; i < segments.length; i++) {
      const segment = segments[i]!;
      const isLast = i === segments.length - 1;
      const path = segments.slice(0, i + 1).join("/");

      if (!current.children.has(segment)) {
        current.children.set(segment, {
          name: segment,
          path,
          children: new Map(),
        });
      }

      const child = current.children.get(segment)!;
      if (isLast && !file.isDirectory) {
        child.artifact = {
          id: file.path,
          displayName: file.path.split("/").pop() ?? file.path,
          contentType: file.contentType,
          sizeBytes: file.sizeBytes,
        };
      }
      current = child;
    }
  }

  const children = [...root.children.values()].map(toFileNode);
  return sortNodes(children);
};

/** Build a hierarchical file tree from workspace file listing. */
export const useWorkspaceFileTree = (files: WorkspaceFileDisplay[]): FileNode[] =>
  useMemo(() => buildWorkspaceTree(files), [files]);

/** Check whether a file is a text-like type (for syntax highlighting). */
export const isTextContent = (contentType?: string): boolean => {
  if (!contentType) return false;
  return (
    contentType.startsWith("text/") ||
    contentType === "application/json" ||
    contentType === "application/xml" ||
    contentType === "application/javascript" ||
    contentType === "application/typescript"
  );
};

/** Check whether a file can be previewed inline. */
export const canPreview = (contentType?: string): boolean =>
  contentType?.startsWith("image/") === true ||
  contentType === "application/pdf" ||
  isTextContent(contentType);

export const formatSize = (bytes: bigint | number): string => {
  const n = typeof bytes === "bigint" ? Number(bytes) : bytes;
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
};
