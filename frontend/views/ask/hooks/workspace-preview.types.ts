import type { ArtifactDisplay } from "@/views/ask/hooks/use-file-tree";

export type PreviewState = {
  artifact: ArtifactDisplay;
  url: string;
  contentType: string;
  textContent?: string;
};

export type PdfNavigation = {
  page: number;
  pageCount: number | null;
  onPageChange: (page: number) => void;
  onDocumentLoad: (pageCount: number) => void;
};
