import { User } from "lucide-react";

export const UserMessage = ({ content }: { content: string }): React.ReactElement => (
  <div className="flex gap-3">
    <div className="flex size-7 shrink-0 items-center justify-center rounded-full bg-primary text-primary-foreground">
      <User className="size-3.5" />
    </div>
    <div className="min-w-0 flex-1 pt-0.5">
      <p className="text-sm whitespace-pre-wrap">{content}</p>
    </div>
  </div>
);
