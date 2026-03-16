import { AlertCircle, Ban, CheckCircle2, Loader2 } from "lucide-react";
import { createElement } from "react";

export type StatusStyle = {
  label: string;
  variant: "default" | "secondary" | "destructive";
  icon: React.ReactNode;
};

export const defaultStatus: StatusStyle = {
  label: "Running",
  variant: "default",
  icon: createElement(Loader2, { className: "size-3 animate-spin" }),
};

export const statusConfig: Record<string, StatusStyle> = {
  completed: {
    label: "Completed",
    variant: "secondary",
    icon: createElement(CheckCircle2, { className: "size-3" }),
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
};

export type StatusFilter = "all" | "completed" | "failed" | "cancelled" | "running";
