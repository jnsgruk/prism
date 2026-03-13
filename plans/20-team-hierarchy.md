# 20 — Team Hierarchy: Orgs, Groups, Teams, and Squads

## Problem

The Canonical staff directory exports a single "Group" field per person (e.g. "Charm Engineering", "Ubuntu Engineering"). We import this flat value into `org.teams` as the team name. But in reality these groups are **groups** — large organisational units containing many **teams**, each of which may contain **squads**:

```
Canonical (company)
└── Engineering              ← Org (led by a CTO/SVP, contains all groups)
    ├── Charm Engineering        ← Group (led by a VP)
    │   ├── Juju                 ← Team (led by a manager or director, or interim director)
    │   │   ├── Juju Core        ← Squad (led by a manager or interim manager)
    │   │   └── Juju Plugins     ← Squad
    │   ├── Observability        ← Team
    │   │   ├── Service Mesh     ← Squad
    │   │   └── Alerting         ← Squad
    │   ├── Data                 ← Team
    │   └── ...
    ├── Ubuntu Engineering       ← Group
    │   ├── Server               ← Team
    │   ├── Foundations           ← Team
    │   ├── NoSQL                ← Team
    │   └── ...
    ├── Devex                    ← Group
    ├── Devices Engineering      ← Group
    └── Web Engineering          ← Group
```

Today we have 5 "teams" with 180 people, which is useless for meaningful metrics. We need the real hierarchy so that:

1. Metrics are computed at the **team/squad** level where they're actionable
2. Group and org-level views can **aggregate** upward from teams and squads
3. The UI lets you **drill down**: Org → Group → Team → Squad → Person

## Current State

### What the schema already supports

The database schema is actually well-prepared for this:

- `org.teams.parent_team_id` — self-referential FK, allows arbitrary nesting
- `org.teams.org_name` — top-level organisation name
- `org.teams.lead_id` — FK to `org.people` for the team lead
- `ListTeams` RPC accepts `parent_team_id` filter

### What's missing

| Gap                                              | Detail                                                                                                                                                                                              |
| ------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **No team depth/type semantics**                 | The schema supports hierarchy but nothing distinguishes an "org" from a "group" from a "team" from a "squad". A `depth` or `team_type` field would clarify intent.                                   |
| **Directory import is flat**                     | The HTML parser extracts `group` → maps to a single team. No hierarchy is built. People are assigned to the top-level group, not their actual team/squad.                                           |
| **No way to define sub-teams in the UI**         | The frontend has no affordance for creating teams, nesting teams, or assigning managers.                                                                                                            |
| **No aggregation in metrics**                    | `metrics.team_snapshots` stores metrics per `team_id`. No roll-up logic to sum child team metrics into a parent.                                                                                    |
| **`org_name` is redundant with top-level teams** | Currently `org_name` is a string on every team row ("Canonical"). With proper hierarchy, the root of the tree _is_ the group — `org_name` becomes either the company name or the root group's name. |

## Where does sub-team data come from?

The directory HTML only gives us the 5 top-level groups. We need other sources for the actual team/squad structure:

1. **GitHub Teams** — Canonical's GitHub org has team structures defined. The `canonical-repo-automation` repo maps GitHub teams to repos. We already have `github_team_slug` on the teams table. This is the richest existing source.
2. **Manual definition via UI** — an admin creates the team tree and assigns people to leaf teams. Simplest to build, most flexible.
3. **JSON/YAML team definition file** — a structured file that defines the hierarchy, imported alongside the directory HTML. Could be maintained in a config repo.
4. **Directory HTML tree structure** — the directory HTML actually encodes the full reporting tree. Each person is nested in `<ol>` elements that reflect their reporting depth, and every person has a `Manager: <a href="/people/...">Name</a>` field. This gives us both the nesting depth and explicit manager→report relationships.

**What the directory HTML nesting tells us:**

| `<ol>` depth | Role pattern | Maps to |
|---------------|-------------|---------|
| 1 | VP | Group leader (root of a Group) |
| 2 | Directors, Senior Managers, Managers | Team leaders (direct reports to VP) |
| 3 | Managers, Interim Managers | Squad leaders (when they have reports) or ICs |
| 4+ | ICs | Squad/team members |

For Charm Engineering, the directory reveals this structure:

```
Jon Seager (VP) ← Group                               [depth 1, 19 total reports]
├── Matthieu Clemenceau (Director) ← Team              [depth 2, 9 reports]
│   ├── Julian Klode (Interim Manager) ← Squad         [depth 3, 3 reports]
│   ├── Samir Kamerkar (Director) ← Team               [depth 3, 7 reports]
│   │   └── Scott McNew (Manager) ← Squad              [depth 4, 5 reports]
│   └── Ravi Kant Sharma (Manager) ← Squad             [depth 3, 5 reports]
├── Ben Hoyt (Manager) ← Team                          [depth 2, 9 reports]
├── Simon Aronsson (Senior Manager) ← Team             [depth 2, 5 reports]
│   ├── Leon Mintz (Manager) ← Squad                   [depth 3, 3 reports]
│   └── Pietro Pasotti (Interim Manager) ← Squad       [depth 3, 4 reports]
├── Cristovao Cordeiro (Manager) ← Team                [depth 2, 6 reports]
├── Mykola Marzhan (Director) ← Team                   [depth 2, 3 reports]
│   ├── Marc Oppenheimer (Interim Manager) ← Squad     [depth 3, 2 reports]
│   └── Mehdi Bendriss (Manager) ← Squad               [depth 3, 7 reports]
├── Sinan Awad (Director) ← Team                       [depth 2, 12 reports]
│   ├── Vitaly Antonenko (Manager) ← Squad             [depth 3, 6 reports]
│   └── Ales Stimec (Manager) ← Squad                  [depth 3, 4 reports]
├── Daniel Steinbrook (Manager) ← Team                 [depth 2, 7 reports]
├── Enrico Deusebio (Manager) ← Team                   [depth 2, 8 reports]
├── Alex Lutay (Manager) ← Team                        [depth 2, 4 reports]
│   └── Paulo Machado (Interim Manager) ← Squad        [depth 3, 3 reports]
├── Alessandro Cabbia (Interim Manager) ← Team         [depth 2, 6 reports]
├── Dmitry Lyfar (Manager) ← Team                      [depth 2, 2 reports]
└── Goran Stojanoski (Manager) ← Team                  [depth 2, 2 reports]
```

**The key insight:** depth-2 people with reports are **team** leaders. Depth-3 people with reports are **squad** leaders. ICs at any depth are assigned to the nearest manager above them. The directory doesn't name the teams/squads explicitly — they're identified by their leader — but we can either auto-name them (e.g. "Matthieu Clemenceau's Team") or prompt the admin to name them during import.

**Recommendation:** Option 4 (directory parsing) is far richer than initially assumed and should be the **primary** import mechanism. Enhance the HTML parser to extract `<ol>` nesting depth and the `--manager` field, then build the hierarchy automatically. Support option 2 (manual UI) for renaming and adjustments. Option 3 (YAML) becomes a fallback for orgs without a structured directory.

## Proposed Changes

### Phase A: Data Model Refinements

#### A1. Add `team_type` enum

Introduce a `team_type` column to `org.teams` to give semantic meaning to each level:

```sql
-- Migration: add team_type to org.teams
CREATE TYPE org.team_type AS ENUM ('org', 'group', 'team', 'squad');
ALTER TABLE org.teams ADD COLUMN team_type org.team_type NOT NULL DEFAULT 'team';
```

| Type    | Meaning                                       | Has parent? | Has children?       |
| ------- | --------------------------------------------- | ----------- | ------------------- |
| `org`   | Top-level org (e.g. "Engineering")            | No (root)   | Yes (groups)        |
| `group` | VP-led group (e.g. "Charm Engineering")       | Yes (org)   | Yes (teams)         |
| `team`  | A manager's team (e.g. "Observability")       | Yes (group) | Optionally (squads) |
| `squad` | Leaf unit within a team (e.g. "Service Mesh") | Yes (team)  | No                  |

Enforcement: `team_type = 'org'` ⟹ `parent_team_id IS NULL`. `team_type = 'group'` ⟹ parent must be `team_type = 'org'`. `team_type = 'squad'` ⟹ parent must be `team_type = 'team'`. This can be enforced at the application layer (repo methods) rather than as a DB constraint, to keep flexibility.

#### A2. Decide on `org_name`

Keep `org_name` as the **company** name (e.g. "Canonical"). The root `org`-type entry (e.g. "Engineering") sits directly under the company, and `group`-type entries sit within it. This lets Prism support multiple companies or multiple orgs within a company in future without schema changes.

#### A3. People belong to leaf teams

Today people are assigned to the top-level group. Going forward:

- People are members of **leaf teams** (the most specific team/squad they belong to)
- Membership in parent teams is **derived** by walking up `parent_team_id`
- The UI shows the full breadcrumb: Org → Group → Team → Squad

### Phase B: Team Management UI

#### B1. Team tree view

Replace the current flat team table with a tree/accordion view:

```
Engineering (180 people)                  [org]
  ├── Charm Engineering (90 people)       [group]
  │   ├── Juju (24 people)               [team]
  │   │   ├── Juju Core (12 people)      [squad]
  │   │   └── Juju Plugins (12 people)   [squad]
  │   ├── Observability (18 people)      [team]
  │   │   ├── Service Mesh (6 people)    [squad]
  │   │   └── Alerting (6 people)        [squad]
  │   └── Data (15 people)              [team]
  ├── Ubuntu Engineering (69 people)     [group]
  │   ├── Server                         [team]
  │   ├── Foundations                     [team]
  │   └── NoSQL                          [team]
  └── Devex (13 people)                  [group]
```

Clicking a group shows its teams. Clicking a team shows its squads and members. Each level shows aggregated metrics.

#### B2. CRUD operations for teams

Add RPCs and UI for:

- `CreateTeam(name, team_type, parent_team_id, lead_id)` — create a new team at any level
- `UpdateTeam(id, name, lead_id, parent_team_id)` — rename, change lead, reparent
- `DeleteTeam(id)` — remove (must have no children or members, or offer to reassign)
- `MoveTeamMember(person_id, from_team_id, to_team_id)` — reassign a person

#### B3. Bulk team structure import

Support importing a YAML/JSON file that defines the hierarchy:

```yaml
# team-structure.yaml
orgs:
  - name: Engineering
    groups:
      - name: Charm Engineering
        teams:
          - name: Juju
            lead_email: alice@canonical.com
            github_team_slug: juju
            squads:
              - name: Juju Core
                lead_email: bob@canonical.com
              - name: Juju Plugins
          - name: Observability
            lead_email: carol@canonical.com
            squads:
              - name: Service Mesh
              - name: Alerting
          - name: Data
      - name: Ubuntu Engineering
        teams:
          - name: Server
          - name: Foundations
          - name: NoSQL
```

The import creates the tree and optionally reassigns people from the flat group to their actual leaf team (matching on a secondary field or requiring manual mapping).

### Phase C: Metrics Aggregation

#### C1. Compute at leaf level

Metrics should be computed for **leaf teams only** (squads, or teams with no children). This is where the meaningful work happens.

#### C2. Roll-up queries

Add repo methods that aggregate metrics upward:

- Team-level metrics = SUM/AVG across its squads
- Group-level metrics = SUM/AVG across its teams
- Org-level metrics = SUM/AVG across its groups

These can be computed on-the-fly from the existing `team_snapshots` table — no new snapshot rows needed for parent teams. The `CompareTeams` RPC would accept a team ID at any level and return rolled-up metrics.

#### C3. Drill-down in the UI

The metrics comparison chart should support drilling into a team to see its sub-teams compared, all the way down to individual contributors.

### Phase D: Enhanced Directory Import (Later)

#### D1. Two-step import flow

1. Import people from directory HTML → assigned to flat groups (as today)
2. Import team structure from YAML → creates hierarchy, reassigns people to leaf teams

#### D2. GitHub team sync

Sync team structure from GitHub org teams API. Match `github_team_slug` to existing teams. Auto-assign people based on GitHub team membership + platform identity mapping.

## Migration Path

The existing flat data becomes `group`-type entries under a new `org`-type root. No data loss:

```sql
-- 1. Create the root org entry
INSERT INTO org.teams (id, name, org_name, team_type)
VALUES (gen_random_uuid(), 'Engineering', 'Canonical', 'org');

-- 2. Backfill existing teams as type 'group' and reparent under the org
UPDATE org.teams
SET team_type = 'group',
    parent_team_id = (SELECT id FROM org.teams WHERE team_type = 'org' AND org_name = 'Canonical')
WHERE parent_team_id IS NULL AND team_type = 'team';
```

Then sub-teams are created underneath groups via UI or YAML import, and people are gradually reassigned from the group to their actual leaf teams.

## Proto Changes

```protobuf
enum TeamType {
  TEAM_TYPE_UNSPECIFIED = 0;
  TEAM_TYPE_ORG = 1;
  TEAM_TYPE_GROUP = 2;
  TEAM_TYPE_TEAM = 3;
  TEAM_TYPE_SQUAD = 4;
}

message Team {
  string id = 1;
  string name = 2;
  string org_name = 3;
  optional string parent_team_id = 4;
  optional string lead_id = 5;
  optional string github_team_slug = 6;
  int32 member_count = 7;        // direct members
  TeamType team_type = 8;        // NEW
  int32 total_member_count = 9;  // NEW: includes descendants
  repeated Team children = 10;   // NEW: for tree responses
}

message CreateTeamRequest {
  string name = 1;
  TeamType team_type = 2;
  optional string parent_team_id = 3;
  optional string lead_id = 4;
  optional string github_team_slug = 5;
}
```

## Decisions

1. **Enforce strict nesting.** Org→group→team→squad strictly. No arbitrary depth.
2. **Allow membership at any level.** Directors/managers can be members of group or team-level entries. ICs default to leaf teams/squads.
3. **Membership date ranges are sufficient** for tracking historical team structure. Team renames/reparents are infrequent enough to handle manually.
4. **Directory HTML is the primary source for hierarchy.** The `<ol>` nesting and `--manager` field encode the full reporting tree. The import should parse this to auto-create the group→team→squad structure, with an admin review step for naming teams (since the directory identifies teams by leader, not by name).

5. **Auto-name teams/squads after their leader.** E.g. "Matthieu Clemenceau's Team". Admins can rename via the UI afterwards.
6. **People who don't clearly belong to a team are flagged, not forced.** Some depth-2 people are senior ICs (Staff Engineers, Fellows) who report directly to the VP but aren't team leaders. The import should flag these in the UI for admin review rather than silently assigning them. Individual metrics will still be available regardless of team assignment.
7. **ICs reporting to directors with mixed reports are members of the team directly** — no squad wrapper needed.
