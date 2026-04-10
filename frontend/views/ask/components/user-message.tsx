import { FileText } from "lucide-react";

import type { Mention } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";
import { MentionType } from "@ps/api/gen/canonical/prism/v1/reasoning_pb";

export const UserMessage = ({
  content,
  attachedFiles,
  mentions,
  onFileClick,
}: {
  content: string;
  attachedFiles?: string[];
  mentions?: Mention[];
  onFileClick?: (path: string) => void;
}): React.ReactElement => {
  // Filter out files whose names appear inline in the message text
  // (these are @-mentioned references, not uploaded attachments)
  const uploadedFiles = attachedFiles?.filter((path) => {
    const name = path.split("/").pop() ?? path;
    return !content.includes(name);
  });

  // People and team mentions (names already appear inline in the text,
  // but we could display badges in the future if needed).
  const _entityMentions = mentions?.filter((m) => m.type === MentionType.PERSON || m.type === MentionType.TEAM);

  return (
    <div className="flex flex-col items-end gap-1.5">
      {content && (
        <div className="max-w-[85%] rounded-2xl rounded-tr-sm bg-primary px-4 py-2.5 text-primary-foreground">
          <p className="text-sm whitespace-pre-wrap">{content}</p>
        </div>
      )}
      {uploadedFiles && uploadedFiles.length > 0 && (
        <div className="flex flex-wrap justify-end gap-1.5">
          {uploadedFiles.map((path) => {
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
};
