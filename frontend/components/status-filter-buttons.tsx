import { Button } from "@/components/ui/button";
import { RunStatus } from "@ps/api/gen/canonical/prism/v1/common_pb";
import type { StatusFilter } from "@/lib/run-status";

const filters: { value: StatusFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: RunStatus.COMPLETED, label: "Completed" },
  { value: RunStatus.COMPLETED_WITH_WARNINGS, label: "Partial" },
  { value: RunStatus.FAILED, label: "Failed" },
  { value: RunStatus.RUNNING, label: "Running" },
];

export const StatusFilterButtons = ({
  value,
  onChange,
}: {
  value: StatusFilter;
  onChange: (value: StatusFilter) => void;
}): React.ReactElement => (
  <div className="flex items-center gap-1">
    {filters.map((f) => (
      <Button
        key={f.value}
        variant={value === f.value ? "default" : "outline"}
        size="sm"
        onClick={() => onChange(f.value)}
      >
        {f.label}
      </Button>
    ))}
  </div>
);
