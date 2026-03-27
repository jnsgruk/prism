import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/views/ask/hooks/use-artifacts", () => ({
  useDownloadArtifact: (): { download: ReturnType<typeof vi.fn>; isPending: boolean } => ({
    download: vi.fn(),
    isPending: false,
  }),
  usePreviewArtifact: (): {
    preview: ReturnType<typeof vi.fn>;
    isPending: boolean;
    state: null;
    close: ReturnType<typeof vi.fn>;
  } => ({
    preview: vi.fn(),
    isPending: false,
    state: null,
    close: vi.fn(),
  }),
}));

// Import after mock setup
const { ArtifactList } = await import("./artifact-list");

afterEach(cleanup);

describe("ArtifactList", () => {
  it("returns null when artifacts array is empty", () => {
    const { container } = render(<ArtifactList artifacts={[]} />);
    expect(container.innerHTML).toBe("");
  });

  it("renders artifact display names", () => {
    const artifacts = [
      { id: "a1", displayName: "team-report.csv", contentType: "text/csv", sizeBytes: 2048 },
      { id: "a2", displayName: "velocity-chart.png", contentType: "image/png", sizeBytes: 153600 },
    ];
    render(<ArtifactList artifacts={artifacts} />);

    expect(screen.getByText("team-report.csv")).toBeInTheDocument();
    expect(screen.getByText("velocity-chart.png")).toBeInTheDocument();
  });

  it("shows correct size format for bytes", () => {
    const artifacts = [{ id: "a1", displayName: "tiny.txt", sizeBytes: 512 }];
    render(<ArtifactList artifacts={artifacts} />);

    expect(screen.getByText("512 B")).toBeInTheDocument();
  });

  it("shows correct size format for kilobytes", () => {
    const artifacts = [{ id: "a1", displayName: "medium.csv", sizeBytes: 2048 }];
    render(<ArtifactList artifacts={artifacts} />);

    expect(screen.getByText("2.0 KB")).toBeInTheDocument();
  });

  it("shows correct size format for megabytes", () => {
    const artifacts = [{ id: "a1", displayName: "large.zip", sizeBytes: 1572864 }];
    render(<ArtifactList artifacts={artifacts} />);

    expect(screen.getByText("1.5 MB")).toBeInTheDocument();
  });

  it("handles bigint sizeBytes", () => {
    const artifacts = [{ id: "a1", displayName: "big.bin", sizeBytes: BigInt(3072) }];
    render(<ArtifactList artifacts={artifacts} />);

    expect(screen.getByText("3.0 KB")).toBeInTheDocument();
  });

  it("shows preview button for previewable content types", () => {
    const artifacts = [
      { id: "a1", displayName: "chart.png", contentType: "image/png", sizeBytes: 1024 },
    ];
    render(<ArtifactList artifacts={artifacts} />);

    // Should have 2 buttons: preview (Eye) + download
    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(2);
  });

  it("hides preview button when contentType is missing", () => {
    const artifacts = [{ id: "a1", displayName: "unknown.bin", sizeBytes: 1024 }];
    render(<ArtifactList artifacts={artifacts} />);

    // Should have only 1 button: download
    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(1);
  });

  it("hides preview button for non-previewable content types", () => {
    const artifacts = [
      {
        id: "a1",
        displayName: "archive.zip",
        contentType: "application/zip",
        sizeBytes: 1024,
      },
    ];
    render(<ArtifactList artifacts={artifacts} />);

    const buttons = screen.getAllByRole("button");
    expect(buttons).toHaveLength(1);
  });
});
