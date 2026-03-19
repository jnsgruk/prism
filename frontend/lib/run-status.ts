import { AlertCircle, AlertTriangle, Ban, CheckCircle2, Loader2 } from "lucide-react";
import { createElement } from "react";

export type StatusFilter =
  | "all"
  | "completed"
  | "completed_with_warnings"
  | "failed"
  | "cancelled"
  | "running";

export type StatusStyle = {
  label: string;
  variant: "default" | "secondary" | "destructive" | "outline";
  icon: React.ReactNode;
};

export const defaultStatus: StatusStyle = {
  label: "Running",
  variant: "default",
  icon: createElement(Loader2, { className: "size-3 animate-spin" }),
};

export const statusConfig = {
  completed: {
    label: "Completed",
    variant: "secondary",
    icon: createElement(CheckCircle2, { className: "size-3" }),
  },
  completed_with_warnings: {
    label: "Partial",
    variant: "outline",
    icon: createElement(AlertTriangle, { className: "size-3" }),
  },
  failed: {
    label: "Failed",
    variant: "destructive",
    icon: createElement(AlertCircle, { className: "size-3" }),
  },
  cancelled: {
    label: "Cancelled",
    variant: "secondary",
    icon: createElement(Ban, { className: "size-3" }),
  },
  running: defaultStatus,
} satisfies Record<Exclude<StatusFilter, "all">, StatusStyle> as Record<string, StatusStyle>;
