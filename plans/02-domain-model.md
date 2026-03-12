# Domain Model

## Bounded Contexts

The system decomposes into five bounded contexts, each with clear responsibilities.

### 1. Authentication Context

Manages user identity, credentials, and sessions. Intentionally minimal at launch (single admin user), but structured to support multi-user, RBAC, and OIDC/SSO later. See [07-authentication.md](./07-authentication.md) for full design.

**Aggregates:**

- **User** — a person who can authenticate and access the system
  - Has a username, display name, and password hash (Argon2id)
  - Has a role (`admin` initially; future: `viewer`, `manager`, etc.)
  - Optionally linked to an `org.people` record via `person_id`
  - Created via first-run wizard (single admin) or future user management

- **Session** — an active authenticated session
  - Opaque 256-bit token (client holds raw token, DB stores SHA-256 hash)
  - Expires after 7 days
  - Tracks `user_agent` and `ip_address` for future "active sessions" UI

**Key design decisions:**
- Bearer tokens via `Authorization` header, not cookies — idiomatic for gRPC, CSRF-immune
- Session tokens are hashed in the DB (SHA-256) so a leaked sessions table doesn't yield valid tokens
- First-run detection: if `auth.users` is empty, the system is in setup mode — only `GetSetupStatus`, `CompleteSetup`, and `Login` RPCs are accessible
- Auth enforced via async tonic interceptor (`tonic-middleware`) on every RPC

### 2. Organisation Context

Models the corporate structure — who works where, and how that changes over time.

**Aggregates:**

- **Person** — an individual contributor or manager
  - Has identities across platforms (GitHub username, Jira account, Discourse handle, etc.)
  - Belongs to one or more teams (with date ranges)
  - Has a level/role (for peer comparison)
  - Imported from a directory file (existing pattern from contristat)

- **Team** — a group of people working together
  - Has a lead (manager or director)
  - Can be nested: a team can have sub-teams (squads)
  - Belongs to an org (top-level) or to a parent team (squads)
  - Maps to platform-specific groups (e.g. GitHub teams)
  - Membership changes over time (tracked with date ranges)
  - Example hierarchy: Organisation → Director's Team → Manager A's Squad, Manager B's Squad, Manager C's Squad

- **Organisation** — a top-level grouping of teams
  - You run two of these

- **Repository** — a codebase owned by a team
  - Belongs to one or more teams (primary owner + contributors)
  - Has platform metadata (GitHub org/repo, default branch, language)
  - Linked from the GitHub teams config (`canonical-repo-automation`)
  - NOT ingested on a schedule — analysed on demand (see Repository Analysis below)

**Key design decisions:**
- Teams are **self-referential** — a team has an optional `parent_team_id`. This models the director → managers → ICs hierarchy naturally. A team with no parent is a top-level team under an org. A team with a parent is a squad.
- Metrics can be computed at any level: squad, team (aggregating its squads), or org (aggregating all teams). The UI should let you drill up and down.
- Team membership is temporal — we store `(person, team, start_date, end_date)` so historical queries are accurate. People belong to the **leaf** team (squad) they work in; their membership in the parent team is derived.
- Platform identities are a value object on Person, not a separate entity
- Directory file import is the primary mechanism for managing people/teams, supplemented by the GitHub teams config from `canonical-repo-automation`

### 3. Activity Context

Captures raw activities from external platforms. This is the write-heavy side of the system.

**Aggregates:**

- **Contribution** — a unit of work performed by a person on a platform
  - Polymorphic: can be a PR, a review, a Jira ticket, a Discourse post, a mailing list message, etc.
  - Always linked to a Person (via platform identity resolution)
  - Has a timestamp, platform source, and platform-specific ID
  - Stores key metrics inline (e.g. lines changed, time to merge, review depth)
  - May store content for enrichment (PR review comments, discourse post body)
  - Has a lifecycle state where applicable (e.g. PR: open → merged/closed)

- **IngestionRecord** — metadata about what was collected and when
  - Tracks per-source watermarks (last successful cursor/timestamp)
  - Records job runs, durations, errors, rate limit waits

**Contribution subtypes (initial set):**

| Subtype | Platform | Key Fields |
|---------|----------|------------|
| PullRequest | GitHub | state, lines_added, lines_removed, time_to_merge, reviewer_count |
| CodeReview | GitHub | PR ref, comment_count, depth_assessment, sentiment |
| Issue | GitHub | state, labels, time_to_close |
| JiraTicket | Jira | status, story_points, cycle_time, type |
| DiscoursePost | Discourse | topic_ref, reply_count, likes, category |
| DiscourseTopic | Discourse | post_count, views, category, solved |
| LaunchpadBug | Launchpad | status, importance, assignee |
| MailingListMessage | Mailing Lists | thread_ref, is_reply |
| DriveDocument | Google Drive | type, edit_count, collaborators |

### 4. Metrics Context

Computes and caches derived metrics from raw activity data.

**Value Objects:**

- **TeamMetrics** — aggregated metrics for a team over a time period
  - DORA metrics: deployment frequency, lead time, change failure rate, MTTR
  - Flow metrics: throughput, cycle time, WIP, flow efficiency
  - Review metrics: avg review depth, review turnaround time
  - Engagement metrics: cross-platform activity distribution

- **IndividualProfile** — aggregated view of a person's contributions
  - Not for ranking individuals, but for understanding contribution patterns
  - Cross-platform activity summary for a given period
  - Peer comparison context (people at same level)

- **ComparisonSnapshot** — a point-in-time comparison between teams
  - Used for the "start a conversation with a manager" use case
  - Highlights outliers and trends

### 5. Reasoning Context

Handles AI-driven analysis and insights.

- **Enrichment** — AI-generated metadata attached to a contribution
  - Sentiment analysis on reviews
  - Depth/quality assessment on PR reviews
  - Topic classification on discourse posts

- **Insight** — an AI-generated observation or recommendation
  - Generated on demand or periodically
  - Backed by specific data points (traceable)
  - Could be team-level ("Team X's review turnaround has increased 40% this quarter") or individual-level

- **Embedding** — vector representation of contribution content
  - Stored via pgvector
  - Used for similarity search, clustering, pattern detection

## Domain Events (for future consideration)

If we move toward event-driven patterns:

- `ContributionIngested` — new activity recorded
- `ContributionStateChanged` — e.g. PR moved from open to merged
- `IngestionCompleted` — a source finished its collection run
- `MetricsRecalculated` — team/individual metrics recomputed

## Entity Relationships

```
User (auth context)
  └── Session (1:N)
  └── Person (0:1, optional link to org context)

Organisation
  └── Team (1:N, top-level — e.g. a director's team)
        └── Team (0:N, squads — e.g. each manager's team)
              └── TeamMembership (person + date range)
                    └── Person (N:M with leaf teams)
                          └── PlatformIdentity (1:N, value object)
                          └── Contribution (1:N)
                                └── Enrichment (1:1, optional)
                                └── Embedding (1:1, optional)
```

Metrics roll up: squad → parent team → org. A query for "Team X" includes all its squads' data.

## Repository Analysis

A distinct concern from contribution tracking. This is about **the state of codebases**, not the activity of people.

### Use Cases
- **Tool adoption:** "What percentage of repos in Org X have adopted `uv`? Which are still on `tox`?"
- **Practice permeation:** "How many repos have AI tooling configured (CLAUDE.md, .cursor/, copilot)?"
- **Migration tracking:** "Which repos still use `setup.py` instead of `pyproject.toml`?"
- **Standards compliance:** "Do all repos have CI configured? Pre-commit hooks? CODEOWNERS?"
- **Dependency landscape:** "Which repos depend on library X? What versions?"

### Why On-Demand, Not Scheduled

Unlike contributions (which change continuously and must be tracked incrementally), repository state:
- Changes slowly (a migration happens once)
- Is expensive to scan deeply (cloning/fetching repo contents)
- Has questions that vary — you don't know what you'll want to check next quarter
- Is best answered by an agent that can look at actual file contents

### Model

- **Repository** — known repos, linked to teams via the GitHub teams config
- **RepoScan** — a point-in-time analysis of a set of repositories
  - Triggered on demand ("scan all repos for uv adoption")
  - Stores structured results per repo
  - Can be compared over time ("re-run the uv scan, has adoption increased?")
- **ScanRule** — a reusable check (e.g. "has pyproject.toml with `[tool.uv]`")
  - Could be a simple file/content check, or an LLM-driven assessment for fuzzier questions

### How It Works

This is a natural fit for the agentic reasoning layer, backed by **ephemeral analysis containers** scheduled via the k8s API.

**Flow:**
1. User asks a question ("which repos have migrated from tox to uv?")
2. The agentic layer determines what to look for and what tools are needed
3. The API server schedules analysis container(s) as k8s Jobs — each pod:
   - Clones the target repo(s)
   - Runs the analysis using real tools (`git`, `ripgrep`, `tokei`, language-specific tooling)
   - Reports structured results back
4. The agent aggregates results by team/org
5. Results are stored in `repo_scans` / `repo_scan_results` for future comparison

**Why containers, not just the GitHub API:**
- Some analysis needs actual file access (parsing `pyproject.toml`, checking config structure)
- Tools like `tokei` (code stats), `ripgrep` (content search), or language-specific linters give richer answers
- Cloning locally avoids GitHub API rate limits for content-heavy scans
- Resource isolation — a scan of 200 repos won't starve the main system

**The analysis container image** is pre-built with common tools (`git`, `ripgrep`, `tokei`, Python, Go, Rust toolchains as needed). It's a general-purpose "workbench" that the agent drives.

Pre-defined scan rules can be saved and re-run periodically if a migration is actively being tracked.

## Identity Resolution

A critical cross-cutting concern: mapping platform identities to people.

- A person may be `jsmith` on GitHub, `john.smith@canonical.com` on Jira, `john_smith` on Discourse
- The directory file import establishes these mappings
- The ingestion layer uses these mappings to attribute contributions to the correct Person
- Unmapped contributions should be flagged, not silently dropped
