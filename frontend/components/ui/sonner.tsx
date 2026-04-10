import { CircleCheckIcon, InfoIcon, Loader2Icon, OctagonXIcon, TriangleAlertIcon } from "lucide-react";
import { Toaster as Sonner, type ToasterProps } from "sonner";

const Toaster = (props: ToasterProps): React.ReactElement => (
  <Sonner
    position="top-center"
    className="toaster group"
    icons={{
      success: <CircleCheckIcon className="size-4" />,
      info: <InfoIcon className="size-4" />,
      warning: <TriangleAlertIcon className="size-4" />,
      error: <OctagonXIcon className="size-4" />,
      loading: <Loader2Icon className="size-4 animate-spin" />,
    }}
    toastOptions={{
      classNames: {
        success:
          "!bg-green-50 !text-green-800 !border-green-200 dark:!bg-green-950 dark:!text-green-200 dark:!border-green-900",
        warning:
          "!bg-amber-50 !text-amber-800 !border-amber-200 dark:!bg-amber-950 dark:!text-amber-200 dark:!border-amber-900",
        error: "!bg-red-50 !text-red-800 !border-red-200 dark:!bg-red-950 dark:!text-red-200 dark:!border-red-900",
        info: "!bg-blue-50 !text-blue-800 !border-blue-200 dark:!bg-blue-950 dark:!text-blue-200 dark:!border-blue-900",
      },
    }}
    style={
      {
        "--normal-bg": "var(--popover)",
        "--normal-text": "var(--popover-foreground)",
        "--normal-border": "var(--border)",
        "--border-radius": "var(--radius)",
      } as React.CSSProperties
    }
    {...props}
  />
);

export { Toaster };
