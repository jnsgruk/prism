import { render } from "@testing-library/react";
import { describe, expect, it } from "vite-plus/test";

import { DeltaBadge } from "./delta-badge";

const getSpan = (container: HTMLElement): HTMLSpanElement => {
  const el = container.querySelector("span");
  if (!el) throw new Error("Expected span element");
  return el;
};

describe("DeltaBadge", () => {
  it("returns null for zero delta", () => {
    const { container } = render(<DeltaBadge delta={0} />);
    expect(container.firstChild).toBeNull();
  });

  it("shows positive delta with up arrow and green color", () => {
    const { container } = render(<DeltaBadge delta={1.5} />);
    const span = getSpan(container);
    expect(span.textContent).toContain("1.50");
    expect(span.className).toContain("text-emerald-600");
  });

  it("shows negative delta with down arrow and red color", () => {
    const { container } = render(<DeltaBadge delta={-2.3} />);
    const span = getSpan(container);
    expect(span.textContent).toContain("2.30");
    expect(span.className).toContain("text-red-600");
  });

  it("inverts colors when invert=true (positive becomes red)", () => {
    const { container } = render(<DeltaBadge delta={1.5} invert />);
    const span = getSpan(container);
    expect(span.className).toContain("text-red-600");
  });

  it("inverts colors when invert=true (negative becomes green)", () => {
    const { container } = render(<DeltaBadge delta={-1.5} invert />);
    const span = getSpan(container);
    expect(span.className).toContain("text-emerald-600");
  });

  it("formats as percent (rounded, no decimals)", () => {
    const { container } = render(<DeltaBadge delta={3.7} format="percent" />);
    expect(container.textContent).toContain("4");
  });

  it("formats as integer (rounded)", () => {
    const { container } = render(<DeltaBadge delta={15.9} format="integer" />);
    expect(container.textContent).toContain("16");
  });

  it("appends suffix", () => {
    const { container } = render(<DeltaBadge delta={42} format="percent" suffix="%" />);
    expect(container.textContent).toContain("%");
  });

  it("uses absolute value for display (negative delta shows positive number)", () => {
    const { container } = render(<DeltaBadge delta={-3.14} format="decimal" />);
    expect(container.textContent).toContain("3.14");
    expect(container.textContent).not.toContain("-");
  });
});
