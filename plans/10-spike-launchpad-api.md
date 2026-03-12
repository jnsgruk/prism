# Spike: Launchpad API as a Data Source for Engineering Metrics

## Purpose

Evaluate the Launchpad API (api.launchpad.net) as a data source for Prism, specifically for DORA metrics (lead time, change failure rate) and flow metrics (throughput, cycle time, WIP). Deployment frequency is out of scope.

---

## 1. API Overview

### Architecture

Launchpad exposes a RESTful web service at `https://api.launchpad.net/`. The API follows standard REST conventions with versioned endpoints:

- **Stable**: `https://api.launchpad.net/1.0/` -- original stable version
- **Development**: `https://api.launchpad.net/devel/` -- latest features, may change

**Decision: Always use the `devel` API.** It has features we need (e.g. date filtering on merge proposals) that are not in `1.0`. Any breaking changes are manageable given our controlled deployment.

Every object in Launchpad has a canonical URL. Collections return JSON with `entries`, `total_size`, `start`, and `next_collection_link` fields.

### HTTP Methods

| Method | Purpose |
|--------|---------|
| GET | Read resources and collections |
| PUT | Replace an entire resource |
| PATCH | Modify specific fields |
| POST | Named operations (write actions) |
| DELETE | Remove hosted files |

### Authentication

Launchpad uses **OAuth 1.0** (not 2.0) with the PLAINTEXT signature method. The flow:

1. POST to `https://launchpad.net/+request-token` with a `oauth_consumer_key` (your app name) to get a request token.
2. User visits `https://launchpad.net/+authorize-token?oauth_token=<token>` to grant access.
3. POST to `https://launchpad.net/+access-token` to exchange the request token for a permanent access token.

Key details:
- Launchpad does **not** use OAuth consumer secrets. Signatures start with `&`.
- Access tokens do not expire (no refresh flow needed).
- Credentials must be sent in the HTTP `Authorization` header (not query string or body).
- Access levels can be scoped: read-only public, read-write public, read-write including private data. For our use case, **read-only public** is sufficient.
- Unauthenticated requests can access all public data but cannot see private bugs, hidden emails, or private teams.
- For headless/automated services, tokens can be created manually via curl and stored as credentials files.

**Implication for Prism**: We need a service account with a Launchpad user. Create an OAuth token once, store it in our secrets manager. No token refresh logic needed.

### Rate Limits

Launchpad's API documentation does **not publish explicit rate limits**. There is no documented requests-per-minute or requests-per-hour cap. In practice:

- The API is not aggressively rate-limited but can be slow for large collections.
- No `X-RateLimit-*` headers are returned.
- Heavy automated use should implement politeness delays (1-2 seconds between paginated requests).
- ETag-based caching is supported -- use `If-None-Match` to get `304 Not Modified` responses and reduce load.
- Compression is supported via `TE: gzip` or `TE: deflate` headers.

**Recommendation**: Start with a 1-second delay between requests. Monitor for 429 or 503 responses and implement exponential backoff. A 3-6 hour ingestion cycle is very conservative and unlikely to hit any limits.

### Pagination

Collections use query parameters:
- `ws.start` -- zero-based offset into the collection
- `ws.size` -- page size (default 75, appears to cap around 300)

Responses include:
- `total_size` -- total items in the collection
- `next_collection_link` -- full URL for the next page (absent on last page)
- `prev_collection_link` -- full URL for the previous page
- `entries` -- array of resource objects

**Implication**: Follow `next_collection_link` until absent. For large collections (10,000+ bugs on a project), this could mean many round-trips. Use filtering to reduce result sets.

---

## 2. Available Data Types

### Summary Table

| Data Type | API Collection | Metric Relevance | Quality |
|-----------|---------------|-------------------|---------|
| Merge Proposals | `branch_merge_proposal` | Lead time, throughput, cycle time, review turnaround | High |
| Bugs | `bug`, `bug_task` | Change failure rate, cycle time, WIP | High |
| Bug Activity Log | `bug.activity_collection` | Status transition timestamps | Medium |
| Git Repositories | `git_repository` | Context for merge proposals | Supporting |
| Branches (Bazaar) | `branch` | Legacy; same as above for bzr repos | Low priority |
| Persons/Teams | `person`, `team` | Identity mapping | Supporting |
| Blueprints | `specification` | Work item tracking, WIP | Low |
| Questions | `question` | Not relevant to DORA/flow | None |
| Package Publishing | `source_package_publishing_history` | Could indicate deployments, but out of scope | None |

---

## 3. Merge Proposals (Launchpad's PRs)

Merge proposals are the single most valuable data type for engineering metrics.

### Available Fields

From the webhook payload documentation and API reference, a `branch_merge_proposal` includes:

| Field | Type | Description |
|-------|------|-------------|
| `registrant` | person link | Who created the proposal |
| `source_git_repository` | repo link | Source repository |
| `source_git_path` | string | Source branch ref (e.g., `refs/heads/feature-x`) |
| `target_git_repository` | repo link | Target repository |
| `target_git_path` | string | Target branch ref (e.g., `refs/heads/main`) |
| `prerequisite_git_repository` | repo link | Prerequisite repository (if any) |
| `prerequisite_git_path` | string | Prerequisite branch ref |
| `queue_status` | string | See status values below |
| `commit_message` | string | Merge commit message |
| `description` | string | Proposal description |
| `whiteboard` | string | Internal notes |
| `preview_diff` | link | Current diff against target |
| `date_created` | datetime | When the proposal was created |
| `date_review_requested` | datetime | When review was requested |
| `date_reviewed` | datetime | When review was completed |
| `date_merged` | datetime | When the merge happened |

**Note**: The legacy Bazaar fields (`source_branch`, `target_branch`) also exist for bzr-based proposals.

### Queue Status Values

| Status | Meaning |
|--------|---------|
| `Work in progress` | Author is still working on it |
| `Needs review` | Ready for review |
| `Approved` | Reviewer approved, awaiting merge |
| `Rejected` | Reviewer rejected |
| `Merged` | Successfully merged |
| `Superseded` | Replaced by another proposal |

### Related Collections

- **`votes`** (`code_review_vote_reference` collection): Each vote has a reviewer, a review type, a comment, and a status (Approve, Needs Fixing, Abstain, Disapprove, Needs Information, Resubmit).
- **`all_comments`** (`code_review_comment` collection): Review comments with author, date, and message body.
- **`preview_diff`**: The diff object, which includes diff text, line counts, and conflict information.

### Metrics We Can Compute

| Metric | How | Confidence |
|--------|-----|------------|
| **Lead time for changes** | `date_merged - date_created` (or first commit timestamp if available from git) | High |
| **Review turnaround** | `date_reviewed - date_review_requested` | High |
| **Time to merge** | `date_merged - date_review_requested` | High |
| **Review depth** | Count of `all_comments` entries per proposal | High |
| **Throughput** | Count of proposals merged per time period | High |
| **Cycle time** | `date_merged - date_created` (from first work to delivery) | High |
| **WIP** | Count of proposals in `Work in progress` or `Needs review` status at a point in time | Medium |
| **Review iterations** | Count of status transitions between `Needs review` and `Needs Fixing` (requires webhook or polling) | Medium |

### Querying Merge Proposals

Merge proposals can be retrieved via:
- A person's merge proposals: `GET /devel/~username?ws.op=getMergeProposals`
- A git repository's merge proposals: navigating the repository's `landing_candidates` or `landing_targets` collections
- Filtering by `status` parameter
- Filtering by `created_since` and `created_before` (added in devel API version)

### Webhooks

Launchpad supports webhooks for merge proposals on git repositories and branches:
- Event type: `merge-proposal:0.1`
- Sub-scopes: `merge-proposal:0.1::status-change`, `merge-proposal:0.1::push`
- Payload includes `action` ("created", "modified", "deleted") and `old`/`new` dicts with full proposal attributes.

**Implication**: We can use webhooks for near-real-time updates instead of (or in addition to) polling. This is a significant advantage over pure polling.

---

## 4. Bugs

### Bug vs Bug Task

Launchpad separates the concept of a **bug** (the problem) from a **bug task** (the assignment of that bug to a specific project/package). One bug can have multiple tasks (e.g., the same bug affects both a library and a downstream package). Bug tasks carry the status, assignee, and milestone -- they are what we care about for metrics.

### Bug Fields

| Field | Type | Notes |
|-------|------|-------|
| `id` | int | Unique bug ID |
| `title` | string | One-line summary |
| `description` | string | Full description |
| `date_created` | datetime | When reported |
| `date_last_updated` | datetime | Most recent change |
| `owner` | person link | Reporter |
| `tags` | string list | Classification tags |
| `information_type` | enum | Public, Public Security, Private Security, Private, Proprietary, Embargoed |
| `bug_tasks_collection_link` | link | Collection of tasks for this bug |
| `activity_collection_link` | link | Full activity log |
| `messages_collection_link` | link | Comments/messages |

### Bug Task Fields (the metric-rich part)

| Field | Type | Notes |
|-------|------|-------|
| `status` | enum | See statuses below |
| `importance` | enum | Undecided, Wishlist, Low, Medium, High, Critical |
| `assignee` | person link | Who is working on it |
| `milestone` | link | Target release |
| `date_created` | datetime | Task creation |
| `date_left_new` | datetime | When status first changed from New |
| `date_confirmed` | datetime | When confirmed |
| `date_triaged` | datetime | When triaged |
| `date_in_progress` | datetime | When work began |
| `date_fix_committed` | datetime | When fix was committed |
| `date_fix_released` | datetime | When fix was released |
| `date_left_closed` | datetime | When reopened from a closed state |

### Bug Statuses

| Status | Available To | Meaning |
|--------|-------------|---------|
| New | Everyone | Just reported |
| Incomplete | Everyone | Needs more info |
| Confirmed | Everyone | Community confirmed |
| Triaged | Bug supervisor | Ready for development |
| In Progress | Everyone | Developer working on it |
| Fix Committed | Everyone | Fix in codebase |
| Fix Released | Everyone | Fix shipped |
| Won't Fix | Bug supervisor | Acknowledged but won't fix |
| Invalid | Everyone | Not a real bug |
| Opinion | Everyone | Disagreement, considered closed |
| Deferred | Bug supervisor | Fix postponed |

### Bug Activity Log

Each bug has an activity log accessible via `activity_collection_link`. Each entry contains:

| Field | Description |
|-------|-------------|
| `datechanged` | Timestamp of the change |
| `person` | Who made the change |
| `whatchanged` | Field that was modified (e.g., "status", "importance", "assignee") |
| `oldvalue` | Previous value |
| `newvalue` | New value |
| `message` | Optional comment |

### Metrics We Can Compute from Bugs

| Metric | How | Confidence |
|--------|-----|------------|
| **Cycle time (report to fix)** | `date_fix_committed - date_created` or `date_fix_released - date_created` | High -- first-class timestamp fields |
| **Cycle time (work to fix)** | `date_fix_committed - date_in_progress` | High |
| **Change failure rate** | Bugs tagged as regressions / total merged proposals in same period | Medium -- depends on team tagging discipline |
| **WIP** | Count of bugs in `In Progress` status at a point in time | High |
| **Triage latency** | `date_triaged - date_created` | High |
| **Time in status** | Computed from activity log transitions | Medium -- requires reconstructing state machine |

**Key advantage**: Unlike many systems, Launchpad stores explicit timestamps for each major status transition on the bug task itself. We do NOT need to reconstruct timelines from activity logs for the common metrics -- the `date_*` fields give us direct access. The activity log is only needed for non-standard transitions or detailed state machine analysis.

### Querying Bugs

The `searchTasks` method on projects/distributions is the primary search endpoint:
- `status` -- filter by one or more statuses
- `importance` -- filter by importance
- `created_since`, `created_before` -- filter by creation date
- `modified_since` -- filter by last modification date
- `assignee`, `bug_reporter` -- filter by person
- `tags` -- filter by tags
- `has_patch` -- boolean filter
- `linked_branches` -- filter for bugs linked to branches
- `orderby` -- sort by status, importance, datecreated, etc.

---

## 5. Rate Limits and Pagination Constraints

### Rate Limits

- **No documented rate limits.** Launchpad does not publish request quotas.
- In practice, the API is designed for moderate automated use. Projects like Ubuntu CI Engine poll it regularly.
- The main bottleneck is response latency, not rate limiting. Large collections can take several seconds per page.
- HTTP caching (ETags) significantly reduces load on subsequent fetches.

### Pagination Constraints

- Default page size: 75 entries
- Maximum page size: appears to be ~300 (undocumented, but larger values are silently capped)
- Must follow `next_collection_link` for traversal
- `total_size` may be expensive to compute for very large collections

### Estimated Ingestion Budget (3-6 hour cycle)

For a typical Canonical team project:

| Resource | Estimated Volume | Pages (at 75/page) | Requests |
|----------|-----------------|---------------------|----------|
| Active merge proposals | 50-200 | 1-3 | 1-3 |
| Merged proposals (incremental) | 5-20 per cycle | 1 | 1 |
| Open bugs | 200-2000 | 3-27 | 3-27 |
| Modified bugs (incremental) | 10-50 per cycle | 1 | 1 |
| Bug details + activity | 10-50 per cycle | 10-50 | 10-50 |
| **Total per project per cycle** | | | **~20-80** |

At 1 request/second with a 1-second politeness delay, this is 40-160 seconds per project. Even with 10 projects, we are well within a 3-hour cycle.

---

## 6. Incremental Collection Strategy

### Supported Filters for Watermarking

| Endpoint | Filter | Field Filtered |
|----------|--------|---------------|
| `bugs.searchTasks()` | `modified_since` | Bug last-modified date |
| `bugs.searchTasks()` | `created_since` / `created_before` | Bug creation date |
| `branches` | `modified_since_date` | Branch modification date |
| `git_repositories` | `modified_since_date` | Repository modification date |
| `getMergeProposals()` | `created_since` / `created_before` | Merge proposal creation date |
| `people.findPerson()` | `created_after` / `created_before` | Person creation date |

### Recommended Watermark Strategy

**For bugs**: Use `modified_since` with our last successful sync timestamp. This catches status changes, new comments, and reassignments. Store the high-water mark as the `date_last_updated` of the most recently modified bug task we processed.

**For merge proposals**: Use `created_since` for new proposals. For updates to existing proposals, there is no `modified_since` filter on merge proposals directly. Two options:
1. **Webhooks** (preferred): Register a webhook on each tracked repository. Get push notifications for status changes. Store in a queue and process on the next sync cycle.
2. **Full scan of active proposals**: Re-fetch all proposals in non-terminal states (`Work in progress`, `Needs review`, `Approved`) each cycle. Since the active set is typically small (< 100 per project), this is cheap.

**For persons**: Sync incrementally using `created_after`. Person records rarely change; a weekly full sync is sufficient.

### Watermark Storage

Store per-project, per-resource-type:
```
{
  "project": "launchpad",
  "resource": "bug_tasks",
  "last_sync": "2026-03-12T10:00:00Z",
  "watermark": "2026-03-12T09:58:32Z"
}
```

Use a small overlap window (e.g., subtract 5 minutes from the watermark) to guard against clock skew and in-flight changes.

---

## 7. Identity Mapping

### Person Fields

| Field | Description |
|-------|-------------|
| `name` | Launchpad username (unique, URL-safe, e.g., `jsmith`) |
| `display_name` | Human-readable name (e.g., "John Smith") |
| `preferred_email_address_link` | Link to email resource (may be hidden) |
| `web_link` | Profile URL (e.g., `https://launchpad.net/~jsmith`) |

### Lookup Methods

- `getByEmail(email)` -- find a person by email address
- `getByOpenIDIdentifier(identifier)` -- find by OpenID
- `findPerson(text)` -- text search across name, display_name, email
- Team membership is accessible via `participants` collection

### Mapping to Prism Person Records

The best strategy:
1. Use `name` (Launchpad username) as the external identifier for the Launchpad data source.
2. Attempt to match by `preferred_email_address` to existing Person records from other sources (GitHub, Jira, etc.).
3. Fall back to fuzzy matching on `display_name` with manual confirmation.
4. Store the `web_link` for linking back to profiles.

**Caveat**: Email addresses may be hidden for privacy. Authenticated requests with appropriate access can see more emails, but some users hide theirs entirely. We may need a manual mapping table for users who cannot be auto-matched.

---

## 8. Gaps and Limitations

### What We Cannot Get

| Gap | Impact | Workaround |
|-----|--------|------------|
| **No deployment events** | Cannot compute deployment frequency from Launchpad alone | Out of scope per requirements, but would need external source (CI/CD system) |
| **No merge proposal `modified_since` filter** | Cannot do efficient incremental sync of MP status changes | Use webhooks or re-scan active proposals |
| **No diff stats on merge proposals** | Cannot measure change size (lines added/removed) from the API directly | The `preview_diff` resource exists but extracting line counts requires fetching and parsing the diff text |
| **No first-commit timestamp** | Lead time computed from MP creation, not first commit | Could supplement with git log data if we also integrate git directly |
| **Limited activity log on merge proposals** | No equivalent of bug activity log for MP status transitions | Track via webhooks or periodic snapshots |
| **No CI/build results on merge proposals** | Cannot assess build pass/fail rates from Launchpad | Would need integration with the CI system (e.g., GitHub Actions, Jenkins) |
| **Email addresses may be hidden** | Identity mapping may be incomplete | Manual mapping table |
| **No explicit rate limit documentation** | Risk of undocumented throttling | Conservative polling with backoff |
| **Bug activity log may not be paginated efficiently** | Old bugs with hundreds of changes may be expensive to fetch | Only fetch activity for bugs that changed since last sync |
| **Blueprints are lightweight** | Not detailed enough for serious work item tracking; many teams don't use them | Use bugs for WIP tracking instead |
| **Questions are not relevant** | Q&A system; no engineering metric value | Skip entirely |

### API Quirks

- We use the `devel` API exclusively. It has features (like date filtering on merge proposals) that are not in `1.0`. Since we control our deployment, any breaking changes are straightforward to handle.
- OAuth 1.0 with PLAINTEXT signing is unusual and not supported by many modern OAuth libraries out of the box. We'll need a simple custom signer or use `requests-oauthlib`.
- JSON string parameters in POST operations require JSON-style quoting (strings need quotes around them in form data).
- The WADL (Web Application Description Language) document at the API root describes all resources programmatically and could be used for auto-discovery, but it is very large.

---

## 9. Recommendation

### Verdict: Yes, integrate Launchpad -- it is a strong data source for Canonical-centric teams.

### Priority Data Sources (high signal, low effort)

**Tier 1 -- Merge Proposals** (implement first):
- Gives us lead time, throughput, cycle time, review turnaround, and review depth.
- Webhook support means near-real-time updates with minimal polling.
- Rich timestamp fields (`date_created`, `date_review_requested`, `date_reviewed`, `date_merged`) map directly to our metrics.
- Active proposal count gives WIP.

**Tier 2 -- Bug Tasks** (implement second):
- Gives us cycle time (reported-to-fixed), change failure rate proxy (regression-tagged bugs), and WIP.
- Exceptional timestamp coverage: `date_left_new`, `date_confirmed`, `date_triaged`, `date_in_progress`, `date_fix_committed`, `date_fix_released` are all first-class fields. This is better than most issue trackers.
- `modified_since` filter enables efficient incremental sync.
- Activity log provides full audit trail when needed.

**Tier 3 -- Persons** (implement alongside Tier 1):
- Needed for identity mapping.
- Simple one-time sync with periodic refresh.

### What to Skip

- **Blueprints**: Too lightweight, inconsistent adoption.
- **Questions**: Not relevant to engineering metrics.
- **Package publishing history**: Relevant to deployment frequency, which is out of scope.
- **Bazaar branches**: Legacy; focus on git repositories only.

### Implementation Approach

1. **OAuth credentials**: Create a service account, generate a permanent token, store in secrets.
2. **Merge proposal collector**: Poll `getMergeProposals` with `created_since` filter + re-scan active proposals. Register webhooks for real-time status changes.
3. **Bug task collector**: Use `searchTasks` with `modified_since` for incremental sync. Fetch bug details and activity log only for changed bugs.
4. **Person collector**: Bulk sync on first run, then incremental by `created_after`. Match to existing Person records by email, fall back to display name.
5. **Sync cadence**: Every 3 hours for bugs and merge proposals. Daily for persons.

### Estimated Implementation Effort

| Component | Effort |
|-----------|--------|
| OAuth client + credential management | 1-2 days |
| Merge proposal collector + transformer | 3-4 days |
| Bug task collector + transformer | 3-4 days |
| Person sync + identity mapping | 2-3 days |
| Webhook receiver for merge proposals | 2-3 days |
| Testing + edge cases | 2-3 days |
| **Total** | **~2-3 weeks** |

---

## Sources

- [Launchpad Web Service API root](https://api.launchpad.net/)
- [API documentation (devel)](https://launchpad.net/+apidoc/devel.html)
- [API documentation (1.0)](https://api.launchpad.net/1.0/)
- [Launchpad API help](https://documentation.ubuntu.com/launchpad/user/how-to/launchpad-api/)
- [OAuth signing](https://documentation.ubuntu.com/launchpad/user/how-to/launchpad-api/launchpad-web-signing/)
- [Web service explanation](https://documentation.ubuntu.com/launchpad/user/explanation/launchpad-api/launchpad-web-service/)
- [Webhooks](https://documentation.ubuntu.com/launchpad/user/how-to/launchpad-api/webhooks/)
- [Bug statuses](https://documentation.ubuntu.com/launchpad/user/reference/bug-tracker-api/bug-statuses/)
- [Blueprints](https://documentation.ubuntu.com/launchpad/user/explanation/feature-highlights/blueprints/)
- [UCI Engine OAuth setup](https://uci.readthedocs.io/en/latest/oauth.html)
- [getMergeProposals date filtering (mailing list)](https://www.mail-archive.com/launchpad-reviewers@lists.launchpad.net/msg31994.html)
- [Bug activity log feature](https://blog.launchpad.net/bug-tracking/feature-friday-the-bug-activity-log)
- [launchpadlib on PyPI](https://pypi.org/project/launchpadlib/)
