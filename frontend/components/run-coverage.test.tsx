import { RunCoverage } from "@/components/run-coverage";
import { render, screen, fireEvent } from "@testing-library/react";
import { describe, expect, it } from "vitest";

describe("RunCoverage", () => {
  it("shows completed coverage notes and translates the visibility marker", () => {
    render(
      <RunCoverage
        progressJson={JSON.stringify({
          coverage: "visible_activity_only: Private or deleted activity is unavailable.",
        })}
      />,
    );
    expect(screen.getByText("Activity coverage")).toBeInTheDocument();
    expect(screen.getByText("Visible activity only: Private or deleted activity is unavailable.")).toBeInTheDocument();
    expect(screen.queryByText(/visible_activity_only/)).not.toBeInTheDocument();
  });
  it("shows multiple coverage notes and lets users inspect failed targets", () => {
    render(
      <RunCoverage
        progressJson={JSON.stringify({
          coverage: ["Limited to visible repositories.", "Some review comments are unavailable."],
          failed_items: [{ key: "org/repository", error: "Permission denied" }],
        })}
      />,
    );
    expect(screen.getByText("Limited to visible repositories.")).toBeInTheDocument();
    expect(screen.getByText("Some review comments are unavailable.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "1 item could not be collected" }));
    expect(screen.getByText("org/repository")).toBeInTheDocument();
    expect(screen.getByText("Permission denied")).toBeInTheDocument();
  });
  it.each([undefined, "invalid", "null", "[]", '{"coverage":null,"failed_items":[null]}'])(
    "ignores absent or malformed coverage: %s",
    (progressJson) => {
      const { container } = render(<RunCoverage progressJson={progressJson} />);
      expect(container).toBeEmptyDOMElement();
    },
  );
});
