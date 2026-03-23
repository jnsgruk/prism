import { describe, expect, it } from "vitest";

import { TeamType } from "@ps/api/gen/prism/v1/org_pb";
import type { Team } from "@ps/api/gen/prism/v1/org_pb";

import { flattenTeams } from "./team-utils";

const makeTeam = (id: string, children: Team[] = []): Team =>
  ({
    id,
    name: `Team ${id}`,
    orgName: "test",
    memberCount: 0,
    teamType: TeamType.TEAM,
    totalMemberCount: 0,
    children,
  }) as Team;

describe("flattenTeams", () => {
  it("returns empty array for empty input", () => {
    expect(flattenTeams([])).toEqual([]);
  });

  it("returns single-level teams unchanged", () => {
    const teams = [makeTeam("a"), makeTeam("b")];
    const result = flattenTeams(teams);
    expect(result.map((t) => t.id)).toEqual(["a", "b"]);
  });

  it("flattens nested teams depth-first", () => {
    const child = makeTeam("child");
    const parent = makeTeam("parent", [child]);
    const result = flattenTeams([parent]);
    expect(result.map((t) => t.id)).toEqual(["parent", "child"]);
  });

  it("flattens deeply nested trees", () => {
    const grandchild = makeTeam("gc");
    const child = makeTeam("c", [grandchild]);
    const root = makeTeam("r", [child]);
    const result = flattenTeams([root]);
    expect(result.map((t) => t.id)).toEqual(["r", "c", "gc"]);
  });
});
