import { WorkspacePdfFailure, WorkspacePdfLoading } from "@/views/ask/components/workspace-pdf-status";
import { Component, lazy, Suspense, type ReactNode } from "react";

const PdfRenderer = lazy(() => import("@/views/ask/components/workspace-pdf-renderer"));

export type WorkspacePdfPreviewProps = {
  url: string;
  page: number;
  onPageChange?: (page: number) => void;
  onDocumentLoad: (pageCount: number) => void;
  /** Numeric zoom is relative to the fitted container width: 1 is fit, 1.25 enlarges it by 25%. */
  scale?: number | "fit-width";
  onDownload?: () => void;
  className?: string;
};

class PdfErrorBoundary extends Component<{ children: ReactNode; onDownload?: () => void }, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError(): { failed: boolean } {
    return { failed: true };
  }

  render(): ReactNode {
    if (this.state.failed) return <WorkspacePdfFailure onDownload={this.props.onDownload} />;
    return this.props.children;
  }
}

export const WorkspacePdfPreview = (props: WorkspacePdfPreviewProps): React.ReactElement => (
  <PdfErrorBoundary key={props.url} onDownload={props.onDownload}>
    <Suspense fallback={<WorkspacePdfLoading />}>
      <PdfRenderer key={props.url} {...props} />
    </Suspense>
  </PdfErrorBoundary>
);
