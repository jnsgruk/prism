import { Badge } from "@/components/ui/badge";
import { Loader2 } from "lucide-react";

export const ContainerStatus = ({ message }: { message: string }): React.ReactElement => (
  <div className="flex items-center justify-center gap-2 py-8 text-muted-foreground">
    <Loader2 className="size-4 animate-spin" />
    <Badge variant="secondary" className="gap-1">
      <span className="relative flex size-2">
        <span className="absolute inline-flex size-full animate-ping rounded-full bg-primary opacity-75" />
        <span className="relative inline-flex size-2 rounded-full bg-primary" />
      </span>
      {message}
    </Badge>
  </div>
);
