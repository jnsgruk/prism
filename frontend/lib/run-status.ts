import { AlertCircle, AlertTriangle, Ban, CheckCircle2, Loader2 } from "lucide-react";
import { createElement } from "react";

import { RunStatus } from "@ps/api/gen/canonical/prism/v1/common_pb";

export type StatusFilter =
  | "all"
  | RunStatus.COMPLETED
  | RunStatus.COMPLETED_WITH_WARNINGS
  | RunStatus.FAILED
  | RunStatus.CANCELLED
  | RunStatus.RUNNING;

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

export const statusConfig: Record<RunStatus, StatusStyle> = {
  [RunStatus.UNSPECIFIED]: defaultStatus,
  [RunStatus.COMPLETED]: {
    label: "Completed",
    variant: "secondary",
    icon: createElement(CheckCircle2, { className: "size-3" }),
  },
  [RunStatus.COMPLETED_WITH_WARNINGS]: {
    label: "Partial",
    variant: "outline",
    icon: createElement(AlertTriangle, { className: "size-3" }),
  },
  [RunStatus.FAILED]: {
    label: "Failed",
    variant: "destructive",
    icon: createElement(AlertCircle, { className: "size-3" }),
  },
  [RunStatus.CANCELLED]: {
    label: "Cancelled",
    variant: "secondary",
    icon: createElement(Ban, { className: "size-3" }),
  },
  [RunStatus.RUNNING]: defaultStatus,
};
