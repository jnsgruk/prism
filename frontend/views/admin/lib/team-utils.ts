import type { Team } from "@ps/api/gen/canonical/prism/v1/org_pb";

/** Recursively flatten a tree of teams into a flat list. */
export const flattenTeams = (teams: Team[]): Team[] => teams.flatMap((t) => [t, ...flattenTeams(t.children)]);
