import { Button } from "@/components/ui/button";
import type { StatusFilter } from "@/lib/run-status";

const filters: { value: StatusFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "completed", label: "Completed" },
  { value: "completed_with_warnings", label: "Partial" },
  { value: "failed", label: "Failed" },
  { value: "running", label: "Running" },
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
