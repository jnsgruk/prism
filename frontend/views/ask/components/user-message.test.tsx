import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vite-plus/test";

import { UserMessage } from "./user-message";

afterEach(cleanup);

describe("UserMessage", () => {
  it("renders text content", () => {
    render(<UserMessage content="Hello world" />);
    expect(screen.getByText("Hello world")).toBeInTheDocument();
  });

  it("renders attachment chips when attachedFiles provided", () => {
    render(<UserMessage content="Check this" attachedFiles={["uploads/report.pdf"]} />);
    expect(screen.getByText("report.pdf")).toBeInTheDocument();
  });

  it("does not render attachment area when no files", () => {
    const { container } = render(<UserMessage content="Hello" />);
    expect(container.querySelectorAll("[class*='flex-wrap']")).toHaveLength(0);
  });
});
