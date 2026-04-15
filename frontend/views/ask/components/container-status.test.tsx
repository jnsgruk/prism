import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vite-plus/test";

afterEach(cleanup);

import { ContainerStatus } from "./container-status";

describe("ContainerStatus", () => {
  it("renders the message prop text", () => {
    render(<ContainerStatus message="Initialising agent..." />);

    expect(screen.getByText("Initialising agent...")).toBeInTheDocument();
  });

  it("contains a Badge with the message", () => {
    render(<ContainerStatus message="Starting container" />);

    // The Badge renders with a specific role or as a div — check for the text within the badge structure
    const badge = screen.getByText("Starting container").closest("[class*='badge']");
    expect(badge).toBeInTheDocument();
  });

  it("shows spinner", () => {
    const { container } = render(<ContainerStatus message="Warming up" />);

    // Loader2 renders as an SVG with animate-spin class
    const spinner = container.querySelector("svg.animate-spin");
    expect(spinner).toBeInTheDocument();
  });
});
