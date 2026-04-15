import { cleanup, render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { afterEach, describe, expect, it } from "vite-plus/test";

afterEach(cleanup);

import { AnswerContent } from "./answer-content";

const renderWithRouter = (content: string): ReturnType<typeof render> =>
  render(
    <MemoryRouter>
      <AnswerContent content={content} />
    </MemoryRouter>,
  );

describe("AnswerContent", () => {
  it("renders plain markdown text", () => {
    renderWithRouter("The team velocity is **42 points** this sprint.");

    expect(screen.getByText(/42 points/)).toBeInTheDocument();
  });

  it("renders markdown table", () => {
    const markdown = `
| Team | Velocity |
|------|----------|
| Alpha | 42 |
| Beta | 38 |
`;
    const { container } = renderWithRouter(markdown);

    const table = container.querySelector("table");
    expect(table).toBeInTheDocument();
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();
  });

  it("renders internal links as React Router Links", () => {
    const markdown = "See [Team Alpha](/teams/alpha) for details.";
    const { container } = renderWithRouter(markdown);

    const link = container.querySelector("a[href='/teams/alpha']");
    expect(link).toBeInTheDocument();
    expect(link?.textContent).toBe("Team Alpha");
    // Internal links should NOT have target="_blank"
    expect(link?.getAttribute("target")).toBeNull();
  });

  it("renders external links with target=_blank", () => {
    const markdown = "See [GitHub](https://github.com/example) for the repo.";
    const { container } = renderWithRouter(markdown);

    const link = container.querySelector("a[href='https://github.com/example']");
    expect(link).toBeInTheDocument();
    expect(link?.getAttribute("target")).toBe("_blank");
    expect(link?.getAttribute("rel")).toContain("noopener");
  });
});
