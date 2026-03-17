import { Activity } from "lucide-react";

import { Card, CardContent } from "@/components/ui/card";
import { Tooltip as UITooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

export const MetricCard = ({
  label,
  value,
  icon: Icon,
  description,
}: {
  label: string;
  value: string;
  icon: React.ComponentType<{ className?: string }>;
  description?: string;
}): React.ReactElement => (
  <Card>
    <CardContent className="flex items-center gap-3 p-4">
      <div className="rounded-md bg-muted p-2">
        <Icon className="size-4 text-muted-foreground" />
      </div>
      <div className="min-w-0 flex-1">
        <p className="text-2xl font-semibold leading-none">{value}</p>
        <div className="mt-1 flex items-center gap-1">
          <p className="text-xs text-muted-foreground">{label}</p>
          {description && (
            <UITooltip>
              <TooltipTrigger render={<button type="button" className="inline-flex shrink-0" />}>
                <Activity className="size-3 text-muted-foreground/50" />
              </TooltipTrigger>
              <TooltipContent side="bottom" className="max-w-64">
                {description}
              </TooltipContent>
            </UITooltip>
          )}
        </div>
      </div>
    </CardContent>
  </Card>
);
