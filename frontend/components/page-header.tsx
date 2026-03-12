import { Separator } from "@/components/ui/separator";
import { SidebarTrigger } from "@/components/ui/sidebar";

export const PageHeader = ({
  title,
  description,
  actions,
}: {
  title: string;
  description?: string;
  actions?: React.ReactNode;
}) => (
  <header className="flex h-14 shrink-0 items-center gap-2 border-b px-4">
    <SidebarTrigger className="-ml-1" />
    <Separator orientation="vertical" className="mr-2 h-4" />
    <div className="flex flex-1 items-center justify-between">
      <div>
        <h1 className="text-sm font-medium">{title}</h1>
        {description && <p className="text-xs text-muted-foreground">{description}</p>}
      </div>
      {actions && <div className="flex items-center gap-2">{actions}</div>}
    </div>
  </header>
);
