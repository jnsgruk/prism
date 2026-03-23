import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DepthHistogram } from "./depth-histogram";

describe("DepthHistogram", () => {
  it("renders depth labels 1-5", () => {
    const { container } = render(<DepthHistogram distribution={[1, 2, 3, 4, 5]} />);
    // Each depth level has a count label and a depth label, so 10 span elements
    const labels = container.querySelectorAll(".text-\\[10px\\]");
    expect(labels.length).toBe(10); // 5 count + 5 depth labels
  });

  it("renders count labels for each bar", () => {
    const { container } = render(<DepthHistogram distribution={[0, 3, 7, 2, 1]} />);
    const countLabels = container.querySelectorAll(".tabular-nums");
    expect(countLabels[0]?.textContent).toBe("0");
    expect(countLabels[2]?.textContent).toBe("7");
  });

  it("applies minimum height of 4% for zero-count bars", () => {
    const { container } = render(<DepthHistogram distribution={[0, 0, 10, 0, 0]} />);

    // Bars with count 0 should have min 4% height
    const bars = container.querySelectorAll(".rounded-t");
    const zeroBar = bars[0] as HTMLElement;
    expect(zeroBar.style.height).toBe("4%");

    // Max bar should have 100% height
    const maxBar = bars[2] as HTMLElement;
    expect(maxBar.style.height).toBe("100%");
  });

  it("normalizes bar heights relative to max value", () => {
    const { container } = render(<DepthHistogram distribution={[5, 10, 0, 0, 0]} />);

    const bars = container.querySelectorAll(".rounded-t");
    const halfBar = bars[0] as HTMLElement;
    expect(halfBar.style.height).toBe("50%");

    const fullBar = bars[1] as HTMLElement;
    expect(fullBar.style.height).toBe("100%");
  });

  it("assigns correct colors by depth level", () => {
    const { container } = render(<DepthHistogram distribution={[1, 1, 1, 1, 1]} />);

    const bars = container.querySelectorAll(".rounded-t");
    expect((bars[0] as HTMLElement).className).toContain("bg-red-500"); // depth 1
    expect((bars[1] as HTMLElement).className).toContain("bg-orange-400"); // depth 2
    expect((bars[2] as HTMLElement).className).toContain("bg-yellow-400"); // depth 3
    expect((bars[3] as HTMLElement).className).toContain("bg-emerald-400"); // depth 4
    expect((bars[4] as HTMLElement).className).toContain("bg-emerald-600"); // depth 5
  });
});
