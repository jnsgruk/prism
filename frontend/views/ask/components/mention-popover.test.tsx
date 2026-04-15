import type { WorkspaceFileDisplay } from "@/views/ask/hooks/use-file-tree";
import { create } from "@bufbuild/protobuf";
import { act, fireEvent, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vite-plus/test";

import { PersonSchema, TeamSchema } from "@ps/api/gen/canonical/prism/v1/org_pb";
import type { Person, Team } from "@ps/api/gen/canonical/prism/v1/org_pb";
import { renderWithProviders, setupCleanup } from "@ps/test-utils";

import { MentionPopover } from "./mention-popover";

setupCleanup();

const makePerson = (id: string, name: string, teamName?: string): Person =>
  create(PersonSchema, {
    id,
    name,
    email: `${name.toLowerCase()}@example.com`,
    active: true,
    teamName: teamName ?? undefined,
    identities: [],
  });

const makeTeam = (id: string, name: string, memberCount = 5): Team =>
  create(TeamSchema, {
    id,
    name,
    orgName: "Org",
    memberCount,
    totalMemberCount: memberCount,
    teamType: 0,
    children: [],
  });

const makeFile = (path: string, sizeBytes = 1000): WorkspaceFileDisplay => ({
  path,
  sizeBytes,
  isDirectory: false,
  contentType: "text/plain",
});

const people = [makePerson("p1", "Alice Smith", "Platform"), makePerson("p2", "Bob Jones", "Core")];
const teams = [makeTeam("t1", "Platform", 12), makeTeam("t2", "Core", 8)];
const files = [makeFile("src/main.rs", 2048), makeFile("README.md", 512)];

const defaultProps = {
  query: "",
  files,
  people,
  teams,
  onSelect: vi.fn<(id: string, name: string, type: string) => void>(),
  onClose: vi.fn<() => void>(),
};

describe("MentionPopover", () => {
  describe("category rendering", () => {
    it("shows People section header when people match", () => {
      renderWithProviders(<MentionPopover {...defaultProps} />);
      expect(screen.getByText("People")).toBeInTheDocument();
    });

    it("shows Teams section header when teams match", () => {
      renderWithProviders(<MentionPopover {...defaultProps} />);
      expect(screen.getByText("Teams")).toBeInTheDocument();
    });

    it("shows Files section header when files match", () => {
      renderWithProviders(<MentionPopover {...defaultProps} />);
      expect(screen.getByText("Files")).toBeInTheDocument();
    });

    it("hides category when no items match", () => {
      renderWithProviders(<MentionPopover {...defaultProps} query="zzz" />);
      expect(screen.queryByText("People")).not.toBeInTheDocument();
      expect(screen.queryByText("Teams")).not.toBeInTheDocument();
      expect(screen.queryByText("Files")).not.toBeInTheDocument();
      expect(screen.getByText("No results")).toBeInTheDocument();
    });

    it("shows No results when nothing matches across all categories", () => {
      renderWithProviders(<MentionPopover {...defaultProps} people={[]} teams={[]} files={[]} query="xyz" />);
      expect(screen.getByText("No results")).toBeInTheDocument();
    });
  });

  describe("filtering", () => {
    it("fuzzy-filters people by name", () => {
      renderWithProviders(<MentionPopover {...defaultProps} query="ali" />);
      expect(screen.getByText("Alice Smith")).toBeInTheDocument();
      expect(screen.queryByText("Bob Jones")).not.toBeInTheDocument();
    });

    it("fuzzy-filters teams by name", () => {
      renderWithProviders(<MentionPopover {...defaultProps} query="plat" />);
      expect(screen.getByText("Platform")).toBeInTheDocument();
      // "Core" should not match "plat"
      expect(screen.queryByText("Core")).not.toBeInTheDocument();
    });

    it("fuzzy-filters files by path", () => {
      renderWithProviders(<MentionPopover {...defaultProps} query="main" />);
      expect(screen.getByText("src/main.rs")).toBeInTheDocument();
      expect(screen.queryByText("README.md")).not.toBeInTheDocument();
    });

    it("empty query shows all items", () => {
      renderWithProviders(<MentionPopover {...defaultProps} query="" />);
      expect(screen.getByText("Alice Smith")).toBeInTheDocument();
      expect(screen.getByText("Bob Jones")).toBeInTheDocument();
      // "Platform" appears both as a team name and as Alice's team subtitle
      expect(screen.getAllByText("Platform").length).toBeGreaterThanOrEqual(1);
      expect(screen.getByText("src/main.rs")).toBeInTheDocument();
    });
  });

  describe("subtitles", () => {
    it("person items show team name", () => {
      renderWithProviders(<MentionPopover {...defaultProps} query="ali" />);
      // The team name "Platform" appears as subtitle for Alice
      const platformTexts = screen.getAllByText("Platform");
      expect(platformTexts.length).toBeGreaterThanOrEqual(1);
    });

    it("team items show member count", () => {
      renderWithProviders(<MentionPopover {...defaultProps} query="plat" />);
      expect(screen.getByText("12 members")).toBeInTheDocument();
    });
  });

  describe("selection", () => {
    it("clicking a person calls onSelect with person type", () => {
      const onSelect = vi.fn<(id: string, name: string, type: string) => void>();
      renderWithProviders(<MentionPopover {...defaultProps} onSelect={onSelect} query="ali" />);
      const button = screen.getByText("Alice Smith").closest("button")!;
      fireEvent.mouseDown(button);
      expect(onSelect).toHaveBeenCalledWith("p1", "Alice Smith", "person");
    });

    it("clicking a team calls onSelect with team type", () => {
      const onSelect = vi.fn<(id: string, name: string, type: string) => void>();
      renderWithProviders(<MentionPopover {...defaultProps} people={[]} files={[]} onSelect={onSelect} query="" />);
      const button = screen.getByText("Platform").closest("button")!;
      fireEvent.mouseDown(button);
      expect(onSelect).toHaveBeenCalledWith("t1", "Platform", "team");
    });

    it("clicking a file calls onSelect with file type", () => {
      const onSelect = vi.fn<(id: string, name: string, type: string) => void>();
      renderWithProviders(<MentionPopover {...defaultProps} people={[]} teams={[]} onSelect={onSelect} query="" />);
      const button = screen.getByText("src/main.rs").closest("button")!;
      fireEvent.mouseDown(button);
      expect(onSelect).toHaveBeenCalledWith("src/main.rs", "main.rs", "file");
    });

    it("selectCurrent imperative call selects highlighted item", () => {
      const onSelect = vi.fn<(id: string, name: string, type: string) => void>();
      const navRef = { current: null } as React.RefObject<{
        moveUp: () => void;
        moveDown: () => void;
        selectCurrent: () => void;
      } | null>;
      renderWithProviders(<MentionPopover {...defaultProps} onSelect={onSelect} onNavigate={navRef} />);
      // First item (Alice) should be selected by default
      navRef.current!.selectCurrent();
      expect(onSelect).toHaveBeenCalledWith("p1", "Alice Smith", "person");
    });
  });

  describe("keyboard navigation", () => {
    it("moveDown crosses category boundary", () => {
      const onSelect = vi.fn<(id: string, name: string, type: string) => void>();
      const navRef = { current: null } as React.RefObject<{
        moveUp: () => void;
        moveDown: () => void;
        selectCurrent: () => void;
      } | null>;
      renderWithProviders(<MentionPopover {...defaultProps} onSelect={onSelect} onNavigate={navRef} />);
      // Move past all people (2), into teams
      act(() => navRef.current!.moveDown());
      act(() => navRef.current!.moveDown());
      act(() => navRef.current!.selectCurrent());
      expect(onSelect).toHaveBeenCalledWith("t1", "Platform", "team");
    });

    it("moveUp wraps from first to last item", () => {
      const onSelect = vi.fn<(id: string, name: string, type: string) => void>();
      const navRef = { current: null } as React.RefObject<{
        moveUp: () => void;
        moveDown: () => void;
        selectCurrent: () => void;
      } | null>;
      renderWithProviders(<MentionPopover {...defaultProps} onSelect={onSelect} onNavigate={navRef} />);
      // Move up from first item should wrap to last
      act(() => navRef.current!.moveUp());
      act(() => navRef.current!.selectCurrent());
      // Last item is the second file (README.md)
      expect(onSelect).toHaveBeenCalledWith("README.md", "README.md", "file");
    });
  });
});
