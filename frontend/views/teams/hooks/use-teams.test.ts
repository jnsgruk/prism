import { create } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";

import { TeamSchema, TeamType } from "@ps/api/gen/canonical/prism/v1/org_pb";
import type { Team } from "@ps/api/gen/canonical/prism/v1/org_pb";

import { findTeam, flattenTree, getAncestors, teamTypeBadgeVariant, teamTypeLabel } from "./use-teams";

const makeTeam = (id: string, children: Team[] = []): Team =>
  create(TeamSchema, {
    id,
    name: `Team ${id}`,
    orgName: "test",
    memberCount: 0,
    teamType: TeamType.TEAM,
    totalMemberCount: 0,
    children,
  });

describe("flattenTree", () => {
  it("returns empty array for empty input", () => {
    expect(flattenTree([])).toEqual([]);
  });

  it("flattens single level with depth 0", () => {
    const result = flattenTree([makeTeam("a"), makeTeam("b")]);
    expect(result).toEqual([
      { team: expect.objectContaining({ id: "a" }), depth: 0 },
      { team: expect.objectContaining({ id: "b" }), depth: 0 },
    ]);
  });

  it("tracks depth for nested teams", () => {
    const child = makeTeam("child");
    const parent = makeTeam("parent", [child]);
    const result = flattenTree([parent]);
    expect(result).toEqual([
      { team: expect.objectContaining({ id: "parent" }), depth: 0 },
      { team: expect.objectContaining({ id: "child" }), depth: 1 },
    ]);
  });

  it("handles three levels of nesting", () => {
    const gc = makeTeam("gc");
    const c = makeTeam("c", [gc]);
    const r = makeTeam("r", [c]);
    const result = flattenTree([r]);
    expect(result.map((f) => ({ id: f.team.id, depth: f.depth }))).toEqual([
      { id: "r", depth: 0 },
      { id: "c", depth: 1 },
      { id: "gc", depth: 2 },
    ]);
  });
});

describe("findTeam", () => {
  const gc = makeTeam("gc");
  const c = makeTeam("c", [gc]);
  const r = makeTeam("r", [c]);
  const tree = [r];

  it("finds root team", () => {
    expect(findTeam(tree, "r")?.id).toBe("r");
  });

  it("finds nested team", () => {
    expect(findTeam(tree, "gc")?.id).toBe("gc");
  });

  it("returns undefined for missing ID", () => {
    expect(findTeam(tree, "nonexistent")).toBeUndefined();
  });

  it("returns undefined for empty tree", () => {
    expect(findTeam([], "r")).toBeUndefined();
  });
});

describe("getAncestors", () => {
  const gc = makeTeam("gc");
  const c = makeTeam("c", [gc]);
  const r = makeTeam("r", [c]);
  const tree = [r];

  it("returns path to root (just root)", () => {
    const path = getAncestors(tree, "r");
    expect(path.map((t) => t.id)).toEqual(["r"]);
  });

  it("returns full path to deeply nested team", () => {
    const path = getAncestors(tree, "gc");
    expect(path.map((t) => t.id)).toEqual(["r", "c", "gc"]);
  });

  it("returns empty array for missing ID", () => {
    expect(getAncestors(tree, "missing")).toEqual([]);
  });

  it("returns empty array for empty tree", () => {
    expect(getAncestors([], "r")).toEqual([]);
  });
});

describe("teamTypeLabel", () => {
  it('returns "Org" for ORG', () => {
    expect(teamTypeLabel(TeamType.ORG)).toBe("Org");
  });

  it('returns "Group" for GROUP', () => {
    expect(teamTypeLabel(TeamType.GROUP)).toBe("Group");
  });

  it('returns "Team" for TEAM', () => {
    expect(teamTypeLabel(TeamType.TEAM)).toBe("Team");
  });

  it('returns "Squad" for SQUAD', () => {
    expect(teamTypeLabel(TeamType.SQUAD)).toBe("Squad");
  });

  it('returns "Unknown" for unspecified', () => {
    expect(teamTypeLabel(TeamType.UNSPECIFIED)).toBe("Unknown");
  });
});

describe("teamTypeBadgeVariant", () => {
  it('returns "default" for ORG', () => {
    expect(teamTypeBadgeVariant(TeamType.ORG)).toBe("default");
  });

  it('returns "secondary" for GROUP', () => {
    expect(teamTypeBadgeVariant(TeamType.GROUP)).toBe("secondary");
  });

  it('returns "outline" for TEAM', () => {
    expect(teamTypeBadgeVariant(TeamType.TEAM)).toBe("outline");
  });

  it('returns "outline" for SQUAD', () => {
    expect(teamTypeBadgeVariant(TeamType.SQUAD)).toBe("outline");
  });

  it('returns "secondary" for unspecified', () => {
    expect(teamTypeBadgeVariant(TeamType.UNSPECIFIED)).toBe("secondary");
  });
});
