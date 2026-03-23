import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import { renderWithProviders, setupCleanup } from "@ps/test-utils";

// Mock all tab components to isolate admin page tab switching logic
vi.mock("@/views/admin/components/sources-tab", () => ({
  SourcesTab: (): React.ReactElement => <div data-testid="sources-tab">Sources Content</div>,
}));
vi.mock("@/views/admin/components/teams-tab", () => ({
  TeamsTab: (): React.ReactElement => <div data-testid="teams-tab">Teams Content</div>,
}));
vi.mock("@/views/admin/components/people-tab", () => ({
  PeopleTab: (): React.ReactElement => <div data-testid="people-tab">People Content</div>,
}));
vi.mock("@/views/admin/components/api-tokens-tab", () => ({
  ApiTokensTab: (): React.ReactElement => <div data-testid="tokens-tab">Tokens Content</div>,
}));
vi.mock("@/views/admin/components/handlers-tab", () => ({
  HandlersTab: (): React.ReactElement => <div data-testid="handlers-tab">Handlers Content</div>,
}));
vi.mock("@/views/admin/components/ai-settings-tab", () => ({
  AiSettingsTab: (): React.ReactElement => <div data-testid="ai-tab">AI Content</div>,
}));
vi.mock("@/views/admin/components/system-tab", () => ({
  SystemTab: (): React.ReactElement => <div data-testid="system-tab">System Content</div>,
}));

// Mock react-router useSearchParams
let mockSearchParams = new URLSearchParams();
const mockSetSearchParams = vi.fn();

vi.mock("react-router", async () => {
  const actual = await vi.importActual<typeof import("react-router")>("react-router");
  return {
    ...actual,
    useSearchParams: (): [URLSearchParams, typeof mockSetSearchParams] => [
      mockSearchParams,
      mockSetSearchParams,
    ],
  };
});

const renderPage = async (): Promise<void> => {
  const { default: AdminPage } = await import("./admin-page");
  renderWithProviders(<AdminPage />);
};

describe("AdminPage", () => {
  setupCleanup();

  it("renders page header", async () => {
    await renderPage();

    expect(screen.getAllByText("Admin").length).toBeGreaterThanOrEqual(1);
  });

  it("renders all tab triggers", async () => {
    await renderPage();

    expect(screen.getByRole("tab", { name: /Sources/i })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /Teams/i })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /People/i })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /API Tokens/i })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /Handlers/i })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /AI/i })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /System/i })).toBeInTheDocument();
  });

  it("defaults to sources tab", async () => {
    mockSearchParams = new URLSearchParams();
    await renderPage();

    expect(screen.getByTestId("sources-tab")).toBeInTheDocument();
  });

  it("shows correct tab from URL params", async () => {
    mockSearchParams = new URLSearchParams("tab=ai");
    await renderPage();

    expect(screen.getByTestId("ai-tab")).toBeInTheDocument();
  });

  it("falls back to sources for invalid tab param", async () => {
    mockSearchParams = new URLSearchParams("tab=nonexistent");
    await renderPage();

    expect(screen.getByTestId("sources-tab")).toBeInTheDocument();
  });
});
