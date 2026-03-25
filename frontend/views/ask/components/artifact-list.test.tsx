import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("@/views/ask/hooks/use-artifacts", () => ({
  useDownloadArtifact: (): { download: ReturnType<typeof vi.fn>; isPending: boolean } => ({
    download: vi.fn(),
    isPending: false,
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
      { id: "a1", displayName: "team-report.csv", sizeBytes: 2048 },
      { id: "a2", displayName: "velocity-chart.png", sizeBytes: 153600 },
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
});
