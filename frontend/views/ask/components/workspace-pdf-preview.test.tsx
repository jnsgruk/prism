import { WorkspacePdfPreview } from "@/views/ask/components/workspace-pdf-preview";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vite-plus/test";

vi.mock("@/views/ask/components/workspace-pdf-renderer", () => ({
  default: ({ url }: { url: string }): React.ReactElement => {
    if (url === "blob:broken") throw new Error("Renderer could not initialize");
    return <div>Rendered {url}</div>;
  },
}));

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

describe("WorkspacePdfPreview", () => {
  it("shows a loading status while the renderer is being imported", async () => {
    render(<WorkspacePdfPreview url="blob:one" page={1} onDocumentLoad={vi.fn<() => void>()} />);
    expect(screen.getByRole("status")).toHaveTextContent("Loading PDF");
    expect(await screen.findByText("Rendered blob:one")).toBeInTheDocument();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("contains initialization failures, offers download, and recovers when the URL changes", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    const onDownload = vi.fn<() => void>();
    const { rerender } = render(
      <WorkspacePdfPreview url="blob:broken" page={1} onDocumentLoad={vi.fn<() => void>()} onDownload={onDownload} />,
    );
    expect(await screen.findByRole("alert")).toHaveTextContent("could not be previewed");
    fireEvent.click(screen.getByRole("button", { name: "Download" }));
    expect(onDownload).toHaveBeenCalledOnce();

    rerender(<WorkspacePdfPreview url="blob:two" page={1} onDocumentLoad={vi.fn<() => void>()} />);
    expect(await screen.findByText("Rendered blob:two")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });
});
