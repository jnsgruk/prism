import { FileAttachmentChips } from "@/views/ask/components/file-attachment-chips";

export const UserMessage = ({
  content,
  attachedFiles,
}: {
  content: string;
  attachedFiles?: string[];
}): React.ReactElement => (
  <div className="flex justify-end">
    <div className="max-w-[85%] rounded-2xl rounded-tr-sm bg-primary px-4 py-2.5 text-primary-foreground">
      {content && <p className="text-sm whitespace-pre-wrap">{content}</p>}
      {attachedFiles && attachedFiles.length > 0 && (
        <div className="mt-2 [&_*]:text-primary-foreground/80">
          <FileAttachmentChips
            files={attachedFiles.map((path) => ({
              name: path.split("/").pop() ?? path,
              size: 0,
            }))}
          />
        </div>
      )}
    </div>
  </div>
);
