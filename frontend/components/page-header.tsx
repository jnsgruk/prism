import { Separator } from "@/components/ui/separator";
import { SidebarTrigger } from "@/components/ui/sidebar";

export const PageHeader = ({
  title,
  description,
  center,
  actions,
}: {
  title: React.ReactNode;
  description?: React.ReactNode;
  center?: React.ReactNode;
  actions?: React.ReactNode;
}): React.ReactElement => (
  <header className="flex h-14 shrink-0 items-center gap-2 border-b px-4">
    <SidebarTrigger />
    <Separator orientation="vertical" className="mr-2 h-4" />
    <div className="flex flex-1 items-center justify-between">
      <div className="shrink-0">
        <h1 className="text-sm font-medium">{title}</h1>
        {description && <p className="text-xs text-muted-foreground">{description}</p>}
      </div>
      {center && (
        <div className="flex min-w-0 flex-1 items-center justify-center px-4">{center}</div>
      )}
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </div>
  </header>
);
