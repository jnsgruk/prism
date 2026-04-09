import { FileText } from "lucide-react";

export const UserMessage = ({
  content,
  attachedFiles,
  onFileClick,
}: {
  content: string;
  attachedFiles?: string[];
  onFileClick?: (path: string) => void;
}): React.ReactElement => (
  <div className="flex flex-col items-end gap-1.5">
    {content && (
      <div className="max-w-[85%] rounded-2xl rounded-tr-sm bg-primary px-4 py-2.5 text-primary-foreground">
        <p className="text-sm whitespace-pre-wrap">{content}</p>
      </div>
    )}
    {attachedFiles && attachedFiles.length > 0 && (
      <div className="flex flex-wrap justify-end gap-1.5">
        {attachedFiles.map((path) => {
          const name = path.split("/").pop() ?? path;
          return (
            <button
              type="button"
              key={path}
              onClick={() => onFileClick?.(path)}
              className="inline-flex items-center gap-1.5 rounded-lg border bg-muted/50 px-2.5 py-1 text-xs text-muted-foreground transition-colors hover:bg-muted"
            >
              <FileText className="size-3.5 shrink-0" />
              {name}
            </button>
          );
        })}
      </div>
    )}
  </div>
);
