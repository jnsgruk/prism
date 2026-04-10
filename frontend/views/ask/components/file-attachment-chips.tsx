import { File, FileCode, FileText, Image, X } from "lucide-react";

const formatSize = (bytes: number): string => {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

const getFileIcon = (name: string): React.ReactElement => {
  const ext = name.split(".").pop()?.toLowerCase() ?? "";
  const imageExts = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico"];
  const codeExts = ["ts", "tsx", "js", "jsx", "py", "rs", "go", "java", "rb", "sh", "sql", "c", "cpp", "h"];
  const textExts = ["txt", "md", "csv", "log", "json", "yaml", "yml", "toml", "xml", "html", "css"];

  if (imageExts.includes(ext)) return <Image className="size-3.5 shrink-0" />;
  if (codeExts.includes(ext)) return <FileCode className="size-3.5 shrink-0" />;
  if (textExts.includes(ext) || ext === "pdf") return <FileText className="size-3.5 shrink-0" />;
  return <File className="size-3.5 shrink-0" />;
};

const extractFilename = (path: string): string => path.split("/").pop() ?? path;

export interface AttachedFileInfo {
  name: string;
  size: number;
}

export const FileAttachmentChips = ({
  files,
  onRemove,
}: {
  files: AttachedFileInfo[];
  onRemove?: (index: number) => void;
}): React.ReactElement | null => {
  if (files.length === 0) return null;

  return (
    <div className="flex flex-wrap gap-1.5 px-4 pb-2">
      {files.map((file, index) => (
        <div
          key={`${file.name}-${index}`}
          className="flex items-center gap-1 rounded-md bg-muted px-2 py-1 text-xs text-muted-foreground"
        >
          {getFileIcon(file.name)}
          <span className="max-w-[150px] truncate">{extractFilename(file.name)}</span>
          <span className="text-[10px] opacity-70">{formatSize(file.size)}</span>
          {onRemove && (
            <button
              type="button"
              onClick={() => onRemove(index)}
              className="ml-0.5 rounded-sm p-0.5 hover:bg-muted-foreground/20"
              aria-label={`Remove ${extractFilename(file.name)}`}
            >
              <X className="size-3" />
            </button>
          )}
        </div>
      ))}
    </div>
  );
};
