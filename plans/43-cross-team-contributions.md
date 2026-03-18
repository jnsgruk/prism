# Plan 43: Complete Individual Contribution Capture

## Problem Statement

When viewing an individual's profile, we want to see **all** their GitHub contributions — not just those in repos owned by their team.

Prism's GitHub ingestion has two phases:

1. **TeamRepos** — fetches PRs/reviews for repos owned by mapped GitHub teams
2. **MemberSearch** — searches for PRs authored by team members across all configured orgs, filtering out repos already covered in phase 1

So cross-repo contributions **are** already collected when MemberSearch runs. But there are reliability and completeness gaps that mean individual profiles often have missing work:

### Gap 1: MemberSearch Is Fragile

MemberSearch is skipped entirely when the GitHub rate limit drops below 200 remaining after the TeamRepos phase. When this happens, all cross-repo contributions for that ingestion cycle are silently lost. There's no retry — the next scheduled run starts fresh and may hit the same budget problem.

### Gap 2: Reviews Not Discovered Cross-Repo

MemberSearch only searches by `author:`, so it finds PRs but **not reviews**. If a team member reviews PRs on repos outside their team's repos, those reviews go uncaptured. Reviews are only collected during the TeamRepos phase (where the PR itself is fetched with its reviews inline).

### Gap 3: No Visibility Into What's Missing

When MemberSearch is skipped, the only signal is a `warn!` log line. The ingestion run completes "successfully" — the UI doesn't surface that cross-repo data was skipped. An admin looking at a person's profile has no way to know contributions are incomplete.

## Proposed Solution

### 1. Defer MemberSearch Instead of Skipping

When the rate limit budget is too low for MemberSearch after TeamRepos completes, instead of skipping it entirely:

- Complete the TeamRepos phase and advance the watermark for that phase
- Schedule a **deferred MemberSearch invocation** via Restate's `send_with_delay()` — wait until the rate limit resets (GitHub returns `x-ratelimit-reset` as a Unix timestamp, and we already track it in the GraphQL response)
- The deferred invocation picks up where MemberSearch would have started, using the same watermark

This requires splitting the watermark: TeamRepos and MemberSearch need independent watermarks so that deferring one doesn't block or re-run the other.

**Changes:**
- `activity.ingestion_watermarks`: add a `phase` discriminator (or use separate watermark keys like `github-main:team_repos` and `github-main:member_search`)
- `transition_to_member_search()`: instead of returning an empty `FetchResult` when rate-limited, schedule a delayed self-invocation and return a result indicating "deferred"
- Ingestion run tracking: record that MemberSearch was deferred, not skipped

### 2. Capture Cross-Repo Reviews

Extend MemberSearch to discover reviews on external repos. GitHub's search API doesn't support `reviewed-by:` as a qualifier, so we need an alternative approach.

**Option A: GraphQL ContributionsCollection** (recommended)

Use the `user.contributionsCollection.pullRequestReviewContributions` field to get reviews by a user within a date range. This is a first-class GitHub API specifically for "what has this user done?" and returns reviews across all repos.

```graphql
query($login: String!, $from: DateTime!, $to: DateTime!) {
  user(login: $login) {
    contributionsCollection(from: $from, to: $to) {
      pullRequestReviewContributions(first: 100) {
        nodes {
          pullRequestReview {
            id, state, body, createdAt
            pullRequest { number, title, url, repository { owner { login }, name } }
          }
        }
      }
    }
  }
}
```

- Scoped by date range (use watermark → now)
- Returns reviews across all repos, not just configured orgs
- Filter out reviews on already-ingested team repos (same dedup logic as existing MemberSearch)
- One query per user (not batched like PR search), but reviews are typically fewer

**Option B: User Events API** (fallback)

`GET /users/{user}/events` returns the last 300 events including `PullRequestReviewEvent`. Separate rate limit from the GraphQL API. Limited to recent events only — not suitable for historical backfill.

**Recommendation:** Option A for completeness. The ContributionsCollection API is designed for exactly this use case and respects date ranges.

### 3. Surface Ingestion Completeness

Make it visible when MemberSearch was skipped or deferred:

- Add a `phases_completed` field to ingestion runs (e.g., `["team_repos", "member_search"]` vs `["team_repos"]`)
- The ingestion run detail UI already exists — show which phases ran
- On the individual profile page, if the latest ingestion run didn't include MemberSearch, show a subtle indicator: "Cross-repo contributions may be incomplete — last full scan: {date}"

### 4. Individual Profile Display

The individual profile page already shows all contributions from `list_person_contributions`. Once gaps 1–2 are fixed, the data will be complete. No major UI changes needed beyond:

- Contributions already show the repo name in metadata — this naturally shows work across different repos
- Consider grouping or filtering by repo on the person profile, so it's easy to see the breadth of where someone contributes

## Implementation Order

1. **Split watermarks** — separate TeamRepos and MemberSearch watermarks so they can advance independently
2. **Deferred MemberSearch** — replace skip-on-rate-limit with Restate delayed self-invocation
3. **Phase tracking on runs** — record which phases completed in the ingestion run
4. **Cross-repo review discovery** — add ContributionsCollection query to MemberSearch phase
5. **UI polish** — completeness indicator on profiles, repo grouping/filtering

## Decisions

1. **Watermark strategy** — separate keys (`source:team_repos` / `source:member_search`). Simpler, no schema changes to the watermarks table.

2. **Review discovery** — Option A (ContributionsCollection). One query per team member per run — acceptable cost for complete review coverage.

3. **Deferred invocation delay** — use the exact `x-ratelimit-reset` timestamp from GitHub's response to schedule the deferred MemberSearch invocation.
