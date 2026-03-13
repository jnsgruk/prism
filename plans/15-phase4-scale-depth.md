# Phase 4: Scale & Depth

Detailed implementation plan for the final phase of Prism. Phase 4 assumes Phases 1-3 are complete: multiple sources flowing (GitHub, Jira, Discourse), metrics computed and cached, AI enrichment running at ingestion time, embeddings stored, and agentic querying functional.

**Exit criteria:** The system proactively surfaces insights, covers all planned data sources, and correlates activity across platforms.

**Code structure:** All new code follows feature-first organisation per [18-code-structure.md](./18-code-structure.md). New source adapters (Launchpad, mailing list, Google Drive) go in `ps-ingestion/src/sources/<platform>/`. Cross-platform correlation logic goes in `ps-metrics/src/features/correlation/`. Periodic insight generation goes in `ps-reasoning/src/features/periodic/`. Frontend insight views go in `views/insights/`.

---

## 1. Workstreams

Phase 4 breaks into five parallel workstreams with limited inter-dependencies.

| # | Workstream | Estimated Duration | Can Start Immediately |
|---|-----------|-------------------|----------------------|
| W1 | Periodic Insight Generation | 2-3 weeks | Yes |
| W2 | Launchpad Source | 2-3 weeks | Yes |
| W3 | Mailing List Source | 1-2 weeks | Yes |
| W4 | Google Drive Source | 1-2 weeks (scoping) + TBD | Yes (scoping only) |
| W5 | Cross-Platform Correlation | 2-3 weeks | After W2 reaches Tier 1 |

W1, W2, and W3 are independent and can proceed in parallel from day one. W4 begins as a scoping exercise since the requirements are still open. W5 depends on having Launchpad data flowing (at least merge proposals) so that cross-platform correlation has a meaningful additional source beyond what Phases 1-2 already provide.

---

## 2. Deliverables per Workstream

### W1 — Periodic Insight Generation

| Deliverable | Description |
|------------|-------------|
| Insight scheduler | Restate scheduled handler (cron-like) triggering weekly and monthly insight generation runs |
| Team-level insight generator | Produces 3-5 actionable insights per team per period, stored in `reasoning.insights` |
| Org-level rollup | Aggregates cross-team patterns into director-level insights |
| Insight delivery — UI | Insights page at `/insights` (new top-level nav entry) showing all recent insights filterable by team, scope, and category. Each insight has "show evidence" drill-down. Notification bell in the nav bar shows unread insight count with a dropdown preview linking to the full page. |
| Insight delivery — email digest | Optional weekly email digest per team lead, configurable in `config.global_settings` |
| Insight freshness tracking | Each insight carries a generation timestamp, model attribution, and staleness indicator |
| Admin UI: Insights Settings | New section in the admin panel for insight configuration: enable/disable weekly and monthly schedules, SMTP settings for email digests (host, port, from address), per-team-lead opt-in toggles for individual-level insights, "Generate Now" button to trigger an on-demand insight run for a selected team or org scope. Reads/writes `config.global_settings` under `insights.*` and `email.smtp.*` keys. |

### W2 — Launchpad Source

| Deliverable | Description |
|------------|-------------|
| OAuth 1.0 credential management | Service account token creation, stored via `config.secrets` (encrypted with AES-256-GCM) using `ConfigService.SetSecret`, per the Phase 1 pattern |
| Merge proposal collector | Polls `getMergeProposals` via `devel` API, incremental via `created_since` + active re-scan |
| Merge proposal webhook receiver | HTTP endpoint for Launchpad webhook events (`merge-proposal:0.1`), queued for next sync |
| Bug task collector | Uses `searchTasks` with `modified_since` for incremental sync |
| Person sync | Bulk initial sync, then periodic refresh; feeds identity resolution |
| Contribution mapping | Transforms merge proposals and bug tasks into `activity.contributions` rows |
| Launchpad-specific metrics | Lead time, review turnaround, cycle time, triage latency computed from Launchpad timestamp fields |
| Admin UI: Launchpad source form | Source-specific form in the Data Sources admin tab. Fields: Launchpad instance URL (default `https://api.launchpad.net/devel/`), tracked projects (multi-value input), tracked people (optional). OAuth consumer key and token via `SetSecret` through password-style inputs. Displays the webhook callback URL for registration. "Test Connection" button validates the OAuth credentials by calling `ConfigService.TestConnection`. |

### W3 — Mailing List Source

| Deliverable | Description |
|------------|-------------|
| `mailing_list.rs` source | `Source` trait implementation in `crates/ps-ingestion/src/sources/mailing_list.rs` |
| `MailingListMessage` variant | Added to `ContributionData` enum in `ps-core` |
| Mailing list source config | Entries in `config.source_configs` for each set of tracked lists |
| Threading support | Message threading via `In-Reply-To`, `References` headers, and normalised subject fallback |
| Identity resolution | Email-based matching against `org.people.email` and `org.platform_identities` |
| Admin UI: Mailing list source form | Source-specific form in the Data Sources admin tab. Fields: archive base URL (default `https://lists.ubuntu.com/archives`), list names (multi-value input), lookback months (numeric input). No credentials required (public archives). "Test Connection" button validates the archive URL is reachable and returns a sample list page. |

### W4 — Google Drive Source (Scoping + Skeleton)

| Deliverable | Description |
|------------|-------------|
| Scope document | Written assessment of what Drive signals are worth tracking and what the API supports |
| Source skeleton | `google_drive.rs` implementing the `Source` trait with configuration, but actual collection deferred until scope is resolved |
| API auth spike | Google OAuth 2.0 service account setup, Drive API v3 read access validated |

### W5 — Cross-Platform Correlation

| Deliverable | Description |
|------------|-------------|
| Cross-platform activity timeline | New "Timeline" tab on `/people/[personId]` showing a unified chronological view of activity across all sources (GitHub, Jira, Discourse, Launchpad, mailing lists). Each entry shows platform icon, contribution type, title, and timestamp. Filterable by platform. |
| Cross-source linking heuristics | Rules for linking related items (e.g. Launchpad MP referencing a GitHub PR, Jira ticket linked to a Discourse thread). Detected links appear on `/contributions/[contributionId]` in a "Cross-platform links" section above the Phase 3 similarity panel, showing link type, confidence, and the linked contribution. |
| Correlation-powered insights | New insight types only possible with multi-source data. Appear on `/insights` with a `cross_platform` category badge. |
| Identity resolution improvements | Automated suggestions for unmapped identities based on cross-platform patterns. Surfaced in admin UI as a new "Identity Suggestions" section with accept/reject buttons. Also accessible via `psctl identity-suggestions --review`. |

---

## 3. Dependencies

```
W1 (Periodic Insights)
  ├── Requires: reasoning.insights schema (exists from Phase 3)
  ├── Requires: Restate scheduled handlers (ingestion scheduler pattern from Phase 1)
  └── Requires: Agentic query tools (from Phase 3)

W2 (Launchpad)
  ├── Requires: Source trait + ingestion pipeline (from Phase 1)
  ├── Requires: Identity resolution (from Phase 1)
  └── Independent of W1, W3, W4, W5

W3 (Mailing Lists)
  ├── Requires: Source trait + ingestion pipeline (from Phase 1)
  ├── Requires: Identity resolution (from Phase 1)
  └── Independent of W1, W2, W4, W5

W4 (Google Drive)
  ├── Requires: Source trait (from Phase 1)
  └── Independent; scoping can happen in parallel with everything

W5 (Cross-Platform Correlation)
  ├── Requires: At least 3 sources flowing (GitHub + Jira/Discourse from Phase 2, Launchpad from W2)
  ├── Requires: Identity resolution working (from Phase 1)
  └── Feeds into: W1 (correlation-powered insights)
```

W5 should start its design work immediately but defer implementation until W2 has merge proposals flowing. W1 can ship an initial version using Phase 1-3 sources, then add Launchpad-aware, mailing-list-aware, and correlation-aware insights as W2, W3, and W5 deliver.

---

## 4. Periodic Insight Generation

### What Insights

The system generates insights at three scopes:

**Team-level (weekly):**
- Throughput trends — "Team X merged 40% fewer PRs this week, but average PR size doubled"
- Review health — "Review turnaround for Team Y improved from 48h to 12h since last month"
- Activity gaps — "Three people on Team Z have no Discourse activity in 6 weeks despite being in a Discourse-heavy project"
- Cycle time outliers — "5 Jira tickets on Team W have been in-progress for over 30 days"
- Cross-platform balance — "Team V's Launchpad bug fix rate is 3x their Jira throughput — are tickets being tracked in the right place?"

**Individual-level (weekly, opt-in per team lead):**
- Contribution pattern shifts — "Alice's review volume dropped 60% this sprint while her PR output doubled"
- Platform engagement — "Bob has been active on GitHub and Jira but has no mailing list or Discourse activity this quarter"

**Org-level (monthly):**
- Cross-team comparisons — "Teams A and B work on similar codebases but have a 3x difference in review turnaround"
- Trend detection — "Across all teams, average cycle time has increased 20% over the last quarter"
- Staffing signals — "Team C has the highest WIP-to-headcount ratio in the org"

### Scheduling via Restate

Insight generation runs as a Restate scheduled handler, following the same pattern as ingestion scheduling from Phase 1.

| Schedule | Scope | Trigger |
|----------|-------|---------|
| Weekly (Monday 06:00 UTC) | Team insights | Restate cron-like schedule |
| Weekly (Monday 06:00 UTC) | Individual insights (if enabled) | Same schedule, filtered by config |
| Monthly (1st of month, 06:00 UTC) | Org rollup | Restate cron-like schedule |
| On demand | Any scope | User-triggered from the UI |

Each run is a durable workflow:
1. Fetch the latest metric snapshots for the target scope and period
2. Query relevant contributions and enrichments
3. Call the LLM (Gemini 3.1 Pro via Google, or equivalent via OpenRouter) with structured data + the insight generation prompt
4. Parse the response into individual insights
5. Store each insight in `reasoning.insights` with full traceability (supporting data, reasoning trace, model name)
6. Mark stale insights from the previous period as superseded

### Delivery

**UI notifications:** The frontend polls for new insights (or uses server streaming via Connect) and surfaces them in a notification panel on the dashboard. Each insight has:
- The insight text
- Scope label (team name, person name, org)
- Period covered
- "How was this generated?" expandable section showing the evidence chain and reasoning trace
- Link to drill down into the underlying metrics/contributions

**Email digest (optional):** Configurable per team lead in the admin UI. A weekly email containing:
- Top 3-5 insights for their team
- Links back to the UI for drill-down
- Unsubscribe link

Email delivery uses a simple SMTP integration (not a full email service). Configuration stored in `config.global_settings` under `email.smtp.*` keys.

### Traceability

Every generated insight must satisfy the traceability requirements from [06-ai-reasoning.md](./06-ai-reasoning.md):
- `supporting_data` — JSON array of contribution IDs, metric snapshot IDs, and enrichment IDs that the insight references
- `reasoning_trace` — ordered list of the agent's tool calls and their results
- `model_name` — which model produced the insight
- `input_hash` — hash of the prompt/data sent to the model for reproducibility auditing

An insight that cannot cite specific supporting data must not be surfaced.

---

## 5. Launchpad Source

Full research is documented in [10-spike-launchpad-api.md](./10-spike-launchpad-api.md). This section summarises the implementation plan.

### Key Constraints

- **Always use the `devel` API** (`https://api.launchpad.net/devel/`). The stable `1.0` API lacks date filtering on merge proposals.
- **OAuth 1.0 with permanent tokens.** No token refresh needed. PLAINTEXT signature method. Credentials in the `Authorization` header.
- **Read-only public access** is sufficient.
- **No documented rate limits.** Implement 1-second politeness delay between requests, exponential backoff on 429/503.

### Implementation Tiers

Implementation follows the tiered priority from the spike:

**Tier 1 — Merge Proposals (week 1-2):**
- `LaunchpadSource` struct implementing the `Source` trait
- Collector fetches merge proposals via `getMergeProposals` on tracked projects/people
- Incremental: `created_since` filter for new proposals, re-scan of non-terminal statuses (`Work in progress`, `Needs review`, `Approved`) each cycle
- Maps to `activity.contributions` with `platform = 'launchpad'`, `contribution_type = 'merge_proposal'`
- Metrics stored in the `metrics` JSONB column: `lines_added`, `lines_removed` (from `preview_diff` if available), `time_to_merge_hours`, `review_vote_count`
- Rich timestamps: `date_created`, `date_review_requested`, `date_reviewed`, `date_merged` stored in `metadata` JSONB
- Fetches related `votes` and `all_comments` collections for review depth

**Tier 1b — Webhooks (week 2):**
- HTTP endpoint registered in the API server for Launchpad webhook callbacks
- Handles `merge-proposal:0.1` events (status changes, pushes)
- Queues webhook payloads for processing on the next ingestion cycle (or immediately if the ingestion service is idle)
- Webhook registration managed per tracked git repository via the Launchpad API

**Tier 2 — Bug Tasks (week 2-3):**
- Fetches bug tasks via `searchTasks` with `modified_since` for incremental sync
- Maps to `activity.contributions` with `contribution_type = 'launchpad_bug'`
- Stores the rich timestamp fields directly in `metadata` JSONB:

| Field | Metric Use |
|-------|-----------|
| `date_created` | Bug age, report-to-fix cycle time |
| `date_left_new` | Triage responsiveness |
| `date_triaged` | Triage latency |
| `date_in_progress` | Time-to-start |
| `date_fix_committed` | Work-to-fix cycle time |
| `date_fix_released` | Full cycle time (report to release) |
| `date_left_closed` | Reopen tracking |

- Fetches activity log only for bugs modified since last sync (avoids expensive full-history fetches)

**Tier 3 — Persons (week 1, alongside Tier 1):**
- Bulk sync of person records for tracked projects
- Maps Launchpad usernames to `org.platform_identities` with `platform = 'launchpad'`
- Attempts email-based matching to existing `org.people` records
- Surfaces unmatched identities in the admin UI for manual resolution
- Refresh cadence: daily (person records rarely change)

### Estimated Effort

Per the spike: approximately 2-3 weeks total, broken down as:

| Component | Days |
|-----------|------|
| OAuth client + credential management | 1-2 |
| Merge proposal collector + transformer | 3-4 |
| Bug task collector + transformer | 3-4 |
| Person sync + identity mapping | 2-3 |
| Webhook receiver | 2-3 |
| Testing + edge cases | 2-3 |

---

## 6. Mailing List Source

### Reference Implementation

A working mbox parser exists at `~/code/newsagent/src/tools/mailing_list.rs`. It provides:

- Fetching gzip-compressed monthly mbox files from `https://lists.ubuntu.com/archives/{list_name}/{YYYY-Month}.txt.gz`
- Parsing with the `mail-parser` crate
- Threading via `In-Reply-To`, `References` headers, and normalised subject fallback
- Cross-list deduplication by `Message-ID`

The Prism source adapter reuses this approach but adapts it to the `Source` trait pattern (incremental watermarks, upsert to DB, identity resolution).

### Data Access Pattern

1. Determine which monthly archive files to fetch based on the watermark
2. Download and decompress each `.txt.gz` file
3. Split on mbox `From ` separator, parse each message with `mail-parser`
4. Filter to messages newer than the watermark
5. Thread messages using `In-Reply-To` / `References` / normalised subject
6. Upsert each message as a contribution; update thread metadata

### Watermark Strategy

- **Watermark type:** `DateTime` — the timestamp of the newest message successfully ingested
- **Monthly granularity:** On each run, fetch the current month's archive plus the previous month's (to catch late arrivals). Filter to messages after the watermark timestamp.
- **First run:** Configurable lookback (e.g. 6 months of archives)

### Fields to Extract

| Field | Maps to | Purpose |
|-------|---------|---------|
| `Message-ID` | `platform_id` | Unique identifier |
| `Subject` | `title` | Display |
| `From` | Identity resolution | Person attribution |
| `Date` | `created_at` | Timestamp |
| `In-Reply-To` | `metadata.in_reply_to` | Threading |
| `References` | `metadata.references` | Threading |
| Body text | `content` | For enrichment |
| List name | `metadata.list_name` | Which list |
| Thread root `Message-ID` | `metadata.thread_id` | Thread grouping |

### Source Config Example

```json
{
  "source_type": "mailing_list",
  "name": "mailing-lists-ubuntu",
  "settings": {
    "base_url": "https://lists.ubuntu.com/archives",
    "lists": [
      "ubuntu-devel",
      "ubuntu-release",
      "ubuntu-security-announce"
    ],
    "lookback_months": 6
  }
}
```

### Identity Resolution

Mailing list `From` headers contain name and email (e.g. `Jane Doe <jane.doe@canonical.com>`). Resolution:

1. Extract email address from the `From` header
2. Look up `org.platform_identities` where `platform = 'mailing_list'` and `platform_username` matches the email
3. Fallback: match against `org.people.email` directly (mailing lists are the one source where the person's canonical email is often visible)
4. If unresolved, store with `person_id = NULL` and record both name and email in `metadata`

### Change Radar

Not applicable — Pipermail archives are static monthly files with no activity stream or change-detection endpoint. The monthly archive approach is already efficient: only the current and previous month's files are fetched each cycle, and messages older than the watermark are filtered out client-side. If the target lists migrate to HyperKitty (Mailman 3), the HyperKitty API's thread/email list endpoints with date filters could serve as a basic radar (see [ingestion strategy — Change Radar](./03-data-ingestion-strategy.md#change-radar-events-api) for the general pattern).

### Considerations

- **No authentication required** — Pipermail archives are public HTTP resources. No credentials needed.
- **No rate limits** — but be polite: add a short delay between archive fetches, set a reasonable `User-Agent`
- **Archive availability:** Some months may not have an archive file (404). Handle gracefully, log, move on.
- **Encoding:** mbox files can contain mixed encodings. `mail-parser` handles this, but `String::from_utf8_lossy` is the fallback for raw splitting.
- **Thread deduplication across lists:** The same thread may appear on multiple lists (cross-posting). Deduplicate by `Message-ID` at the contribution level (the `UNIQUE (platform, platform_id)` constraint handles this naturally if `platform_id` is the `Message-ID`).
- **`mail-parser` crate** is the parser. Add it to `ps-ingestion/Cargo.toml` along with `flate2` for gzip decompression.

### New ContributionData Variant

```rust
MailingListMessage(MailingListMessageMetrics), // Phase 4
```

### Key Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Platform naming | `mailing_list` (single platform value) | All lists share the same parser; `metadata.list_name` distinguishes individual lists |
| Parser crate | `mail-parser` + `flate2` | Proven in newsagent reference implementation |

---

## 7. Google Drive Source

### Current Status: Scope Deferred

Per [08-open-questions.md](./08-open-questions.md) (question 6), Google Drive integration scope is deferred. The exact signals and value are unclear. This workstream begins as a scoping exercise.

### What We Would Likely Track

If implemented, the primary signals would be:

| Signal | Description | API Source |
|--------|-------------|-----------|
| Document authorship | Who created which documents, when | Drive API v3 `files.list` with `createdTime`, `owners` |
| Editing activity | Who edited which documents, how often | Drive API v3 `revisions.list` — each revision has an author and timestamp |
| Shared document contribution | Edits to documents owned by others (collaboration signal) | Revisions on files where the editor is not the owner |
| Document type distribution | Docs vs Sheets vs Slides vs other | `mimeType` field on file metadata |
| Folder/team drive activity | Activity scoped to specific shared drives | `driveId` filter on `files.list` |

### Google Drive API Capabilities

- **Drive API v3** is the current version. Well-documented, JSON-based.
- **Authentication:** Google OAuth 2.0 with a service account. The service account needs domain-wide delegation to access files on behalf of users, or users must explicitly share folders with the service account.
- **Revisions API:** `revisions.list` returns individual revisions with author and timestamp, but only for Google Docs/Sheets/Slides (not uploaded files). Revision history can be large for heavily-edited documents.
- **Changes API:** `changes.list` provides a feed of changes across a drive, with a `pageToken`-based cursor — natural fit for incremental ingestion.
- **Limitations:** Revision details (who edited what) require per-file API calls. For a large corpus this could be expensive in terms of API quota. Google imposes per-user and per-project quotas on Drive API calls.

### Open Questions (to Resolve Before Implementation)

1. **What is the actual value?** Does document authorship meaningfully complement code/issue/discussion activity, or is it noise?
2. **Scope boundary:** Do we track all documents, or only those in specific shared drives / folders?
3. **Privacy:** Accessing document revision history may expose content that people consider private. What is the organisational policy?
4. **Domain-wide delegation:** Requires a Google Workspace admin to grant the service account access. Is this available?
5. **Quota:** Google Drive API has a default quota of 12,000 queries per minute per project. Is this sufficient for the number of users and documents we would track?

### Plan

1. Write a scope document answering the open questions above (requires input from stakeholders)
2. If scope is approved: implement a `GoogleDriveSource` following the same `Source` trait pattern
3. Use the Changes API with `pageToken` as the watermark for incremental ingestion
4. Map to `activity.contributions` with `contribution_type = 'drive_document'`

Until the scope is resolved, no implementation work beyond the skeleton and auth spike.

---

## 8. Cross-Platform Correlation

### Foundation: Identity Resolution

Cross-platform correlation builds on the identity resolution established in Phase 1 (see [02-domain-model.md](./02-domain-model.md#identity-resolution)). Each `org.people` record has multiple `org.platform_identities` linking them across GitHub, Jira, Discourse, mailing lists, and now Launchpad. This is the join key for all cross-platform analysis.

### How Correlation Works

**Level 1 — Person-centric timeline:**
All contributions for a person, across all platforms, ordered chronologically. This already works via `activity.contributions` joined through `person_id`. Phase 4 enriches this with Launchpad data and (potentially) Drive data.

**Level 2 — Related-item linking:**
Detect when items across platforms refer to the same work:
- A Launchpad merge proposal description mentions a GitHub PR number or URL
- A Jira ticket links to a Launchpad bug (via URL in a comment or custom field)
- A Discourse post discusses a specific merge proposal (URL extraction from post body)
- A mailing list thread references a bug number (`LP: #123456` pattern)

Implementation: a background job (Restate workflow) that scans contribution `content`, `metadata`, and `url` fields for cross-references using regex patterns and URL matching. Detected links are stored in a new `activity.contribution_links` table.

**Level 3 — Behavioural correlation:**
Statistical analysis of activity patterns:
- Does a person's GitHub activity drop when their Launchpad activity spikes? (platform switching)
- Do teams with high Discourse engagement have better review turnaround?
- Is there a lag between Jira ticket creation and first Launchpad commit?

These correlations feed the insight generation engine (W1) and the agentic query tools.

### Insights Enabled by Cross-Platform Data

| Insight Type | Example | Required Sources |
|-------------|---------|-----------------|
| Platform switching | "Alice spent 80% of her time on Launchpad bugs this month, up from 20% last month" | Any 2+ sources |
| Work tracking consistency | "Team X has 15 Launchpad bugs in-progress but only 3 matching Jira tickets" | Launchpad + Jira |
| Communication patterns | "PRs from Team Y that have a prior Discourse discussion merge 2x faster" | GitHub/Launchpad + Discourse |
| Response time chains | "Average time from bug report to first commit is 5 days, but mailing list discussions about the same bugs start within hours" | Launchpad + mailing lists |
| Holistic contribution profile | "Bob is a top-3 contributor on Discourse and mailing lists but below average on code output — likely in a mentoring/support role" | All sources |
| Bottleneck detection | "Team Z's Jira tickets move to in-progress quickly but Launchpad MPs sit in review for 5+ days" | Jira + Launchpad |

### Identity Resolution Improvements

Phase 4 adds automated identity suggestions based on cross-platform signals:
- Same display name across platforms (fuzzy match)
- Temporal correlation — two unlinked accounts that are always active at the same times
- Email address overlap (where visible)
- Surfaced as suggestions in the admin UI, requiring manual confirmation (never auto-linked)

---

## 9. New Database Schemas and Tables

All new tables live in existing schemas. Migrations follow the established pattern (see [04-database-design.md](./04-database-design.md)).

### `activity` schema additions

```sql
-- Cross-platform links between contributions
CREATE TABLE activity.contribution_links (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_contribution_id UUID NOT NULL
        REFERENCES activity.contributions(id) ON DELETE CASCADE,
    target_contribution_id UUID NOT NULL
        REFERENCES activity.contributions(id) ON DELETE CASCADE,
    link_type TEXT NOT NULL,
    confidence REAL NOT NULL DEFAULT 1.0,
    detected_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_contribution_id, target_contribution_id, link_type)
);
```

`link_type` values: `'url_reference'`, `'bug_number_mention'`, `'jira_key_mention'`, `'same_work_item'`, `'discussion_of'`.

`confidence` ranges from 0.0 to 1.0 — URL matches are 1.0, regex-based bug number matches are lower.

Indexes:

```sql
CREATE INDEX idx_contribution_links_source
    ON activity.contribution_links(source_contribution_id);
CREATE INDEX idx_contribution_links_target
    ON activity.contribution_links(target_contribution_id);
```

### `reasoning` schema additions

```sql
-- Track insight generation runs
CREATE TABLE reasoning.insight_runs (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    scope_type TEXT NOT NULL,
    scope_id UUID NOT NULL,
    period_start DATE NOT NULL,
    period_end DATE NOT NULL,
    status TEXT NOT NULL DEFAULT 'running',
    insights_generated INTEGER DEFAULT 0,
    started_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    completed_at TIMESTAMPTZ,
    error_message TEXT
);
```

### `reasoning.insights` additions

The existing `reasoning.insights` table (from [04-database-design.md](./04-database-design.md)) needs two new columns:

| Column | Type | Purpose |
|--------|------|---------|
| `insight_run_id` | `UUID REFERENCES reasoning.insight_runs(id)` | Links insight to its generation run |
| `superseded_by` | `UUID REFERENCES reasoning.insights(id)` | When a newer insight replaces this one |
| `delivery_status` | `JSONB DEFAULT '{}'` | Tracks delivery state per channel (`{"ui_read": false, "email_sent_at": null}`) |
| `category` | `TEXT NOT NULL` | Classification: `'throughput'`, `'review_health'`, `'activity_gap'`, `'cycle_time'`, `'cross_platform'`, `'staffing'` |

### `config` schema additions

New keys in `config.global_settings`:

| Key | Value Example | Purpose |
|-----|---------------|---------|
| `insights.weekly.enabled` | `true` | Master toggle for weekly insight generation |
| `insights.monthly.enabled` | `true` | Master toggle for monthly insight generation |
| `insights.email.enabled` | `false` | Whether email digests are active |
| `insights.email.smtp_host` | `"smtp.example.com"` | SMTP server for digests |
| `insights.email.smtp_port` | `587` | SMTP port |
| `insights.email.from_address` | `"studio@canonical.com"` | Sender address |

New rows in `config.source_configs` for Launchpad and (eventually) Google Drive sources.

### Webhook storage

```sql
-- Queued webhook events awaiting processing
CREATE TABLE activity.webhook_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_type TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL,
    received_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    processed_at TIMESTAMPTZ,
    status TEXT NOT NULL DEFAULT 'pending'
);
```

Index for the processing loop:

```sql
CREATE INDEX idx_webhook_events_pending
    ON activity.webhook_events(source_type, received_at)
    WHERE status = 'pending';
```

---

## 10. Backup/Restore Extension

Phase 4 adds new sources, cross-platform linking, and periodic insight generation. Extend the backup/restore bundle to include:

- **Launchpad contributions** — all `activity.contributions` rows where `platform = 'launchpad'` (merge proposals, bug tasks)
- **Mailing list contributions** — all `activity.contributions` rows where `platform = 'mailing_list'`, including threading metadata
- **Google Drive contributions** — if implemented, `activity.contributions` rows where `platform = 'google_drive'`
- **New watermarks** — `activity.ingestion_watermarks` rows for Launchpad, mailing list, and Google Drive sources
- **New source configs** — `config.source_configs` rows for Launchpad, mailing lists, and Google Drive (credentials remain in `config.secrets`, already covered by Phase 1)
- **`activity.webhook_events`** — queued and processed webhook events (include pending events so in-flight webhooks aren't lost on restore)
- **`activity.contribution_links`** — cross-platform links detected by the correlation engine. These are expensive to regenerate (requires re-scanning all contribution content), so preserving them avoids a lengthy re-correlation pass.
- **`reasoning.insight_runs`** — insight generation run history, linked to individual insights
- **Updated `reasoning.insights` columns** — `insight_run_id`, `superseded_by`, `delivery_status`, `category` (new Phase 4 columns)
- **Insight config** — `config.global_settings` entries under `insights.*` and `email.smtp.*`

The `PreviewBackup` RPC response should be updated to include Phase 4 counts (e.g. "892 Launchpad merge proposals, 1,204 bug tasks, 6,341 mailing list messages, 47 cross-platform links, 12 insight runs").

---

## 11. Key Decisions and Risks

### Decisions Needed

| Decision | Options | Recommendation | Status |
|----------|---------|----------------|--------|
| Email digest delivery | SMTP direct / third-party service / skip | SMTP direct — simplest, we control the infra | Open |
| Insight staleness threshold | How long before an insight is considered stale | 1 period (weekly insights stale after 7 days) | Open |
| Google Drive scope | Track all docs / shared drives only / skip entirely | Defer until stakeholder input received | Deferred |
| Webhook endpoint exposure | Public URL / tunnel / VPN-only | Requires a publicly routable endpoint for Launchpad callbacks; may need a reverse proxy or ingress rule | Open |
| Cross-platform link confidence threshold | What confidence level to surface links | 0.7 — below this, flag as "possible" but don't include in metrics | Open |

### `psctl` Extensions

Phase 4 adds periodic insights, new sources, and cross-platform correlation. `psctl` gains:

| Command | Description | Backing RPC |
|---------|-------------|-------------|
| `psctl insights [TEAM] [--period MONTH]` | List generated insights for a team or org, with evidence summaries | `reasoning.insights` query |
| `psctl insights generate [--scope team\|org] [--team TEAM]` | Manually trigger an insight generation run | Insight scheduler trigger |
| `psctl links [CONTRIBUTION_ID]` | Show cross-platform links detected for a contribution | `activity.contribution_links` query |
| `psctl identity-suggestions [--review]` | List automated identity resolution suggestions awaiting confirmation | Identity suggestion query |

`psctl insights` is the primary addition — it lets team leads check their latest insights from the terminal and pipe them into other tools. The `generate` subcommand is useful for testing prompt changes or forcing a refresh after a large backfill.

`psctl identity-suggestions --review` streams unconfirmed suggestions interactively, allowing an admin to accept or reject each one from the terminal. This complements the admin UI for bulk identity cleanup.

All new source data (Launchpad, mailing lists) is automatically included in existing commands like `psctl status`, `psctl contributions`, and `psctl backup` — no source-specific commands are needed since the `Source` trait and contribution model are uniform.

### Risks

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Launchpad API instability on `devel` endpoint | Low | Medium — breaking changes could halt ingestion | Pin to known-good behaviour, add integration tests against the live API, monitor for 4xx/5xx spikes |
| Insight quality — LLM produces generic or incorrect insights | Medium | High — misleading insights undermine trust | Require supporting evidence for every insight; confidence scoring; human review workflow before broad rollout |
| Launchpad OAuth 1.0 client complexity in Rust | Low | Low — well-understood protocol | Use the `oauth1` crate or implement minimal PLAINTEXT signer (no HMAC-SHA1 needed) |
| Google Drive privacy concerns | Medium | High — could block the entire workstream | Scope document must address privacy before any implementation |
| Cross-platform identity gaps | Medium | Medium — unresolved identities produce incomplete correlation | Surface unmatched identities prominently; invest in automated suggestion quality |
| Webhook delivery reliability | Medium | Low — webhooks supplement polling, not replace it | Always maintain polling as the primary mechanism; webhooks are an optimisation for freshness |
| Email digest spam perception | Low | Medium — people ignore automated emails | Make opt-in per team lead, keep digest short (max 5 insights), include unsubscribe |
| Insight generation cost at scale | Low | Medium — more teams and sources means more LLM calls | Batch insights per run, use Gemini 3.1 Pro (not more expensive models), monitor cost per run |

---

## Testing Strategy

### Per-workstream automated tests

**W1 — Periodic Insight Generation:**
- Insight generator with mocked `ModelProvider`: verify 3-5 insights produced per team with correct `supporting_data` and `reasoning_trace`
- Staleness: generate insights, then regenerate for same period — verify old insights marked as superseded
- Budget cap: verify insight generation respects daily budget cap (checks `reasoning.api_usage` before calling LLM)
- Traceability: every insight has `model_name`, `input_hash`, `supporting_data` with valid contribution/metric IDs
- Insight run tracking: verify `reasoning.insight_runs` row created with correct scope, period, status, and insight count
- Category classification: verify insights are categorised (`throughput`, `review_health`, `activity_gap`, etc.)
- Email digest: mock SMTP, trigger digest, verify email contains top insights with correct links
- Admin UI: component tests for Insights Settings — schedule toggles, SMTP config, per-team opt-in, "Generate Now" button
- Insights page (`/insights`): component tests for insight list, filtering by team/scope/category, evidence drill-down, notification bell

**W2 — Launchpad Source:**
- Integration tests against recorded Launchpad API responses (`getMergeProposals`, `searchTasks`, person records)
- Merge proposal pipeline: API response → `MergeProposal` contribution → DB upsert with correct `metadata` (rich timestamps) and `metrics`
- Bug task pipeline: API response → `LaunchpadBug` contribution → DB upsert with lifecycle timestamps
- Identity resolution: Launchpad username → `platform_identity` lookup; unmatched → `person_id = NULL` with metadata
- OAuth 1.0 credential: verify PLAINTEXT signature generation, Authorization header format
- Webhook receiver: POST simulated `merge-proposal:0.1` event → verify queued in `activity.webhook_events` → verify processed on next cycle
- Watermark: verify `created_since` filter advances correctly for merge proposals; `modified_since` for bug tasks
- Admin UI form: component test for Launchpad source creation, `SetSecret` for OAuth token, `TestConnection`, webhook URL display

**W3 — Mailing List Source:**
- Integration tests with sample mbox files (compressed and uncompressed)
- Full pipeline: mbox file → parse → thread → `MailingListMessage` contributions → DB upsert
- Threading: verify `In-Reply-To` and `References` headers correctly build thread structure; verify normalised subject fallback
- Cross-list deduplication: same `Message-ID` appearing in two lists produces one contribution row
- Identity resolution: `From` header email → `org.people.email` match; unmatched → `person_id = NULL`
- Watermark: verify monthly archive fetching respects watermark, only processes newer messages
- Encoding: test with mixed-encoding mbox files (UTF-8, ISO-8859-1, etc.)
- Admin UI form: component test for mailing list source creation, list names input, lookback months

**W4 — Google Drive Source (scoping only):**
- API auth spike: verify Google OAuth 2.0 service account can authenticate against Drive API v3
- Source skeleton: verify `GoogleDriveSource` implements `Source` trait (compiles, no runtime logic)

**W5 — Cross-Platform Correlation:**
- Cross-reference detection: seed contributions with URLs/bug numbers in content, verify `activity.contribution_links` rows created with correct `link_type` and `confidence`
- Regex patterns: test `LP: #123456`, GitHub PR URLs, Jira key patterns (`PROJ-123`), Discourse topic URLs
- Confidence scoring: URL matches → 1.0, regex matches → lower confidence, verify threshold filtering
- Identity suggestions: seed two unlinked accounts with same display name, verify suggestion generated with fuzzy match score
- Timeline query: verify chronological ordering across all platforms for a single person
- Timeline UI: component test for "Timeline" tab on `/people/[personId]` — platform filtering, contribution rendering
- Cross-platform links UI: component test for links section on `/contributions/[contributionId]`

### Per-workstream manual testing

**After W1 (Periodic Insights):**
1. Open admin UI → Insights Settings → verify schedule toggles, SMTP config fields, per-team opt-in
2. Enable weekly insights → click "Generate Now" for a specific team
3. Wait for generation to complete → navigate to `/insights`
4. Verify insights appear with team name, category badge, and period
5. Click an insight → verify "show evidence" drill-down shows supporting contributions and metrics
6. Click "How was this generated?" → verify reasoning trace is displayed
7. Check notification bell in nav bar → verify unread count and dropdown preview
8. If SMTP configured: verify email digest received with top insights and UI links
9. Try `psctl insights [TEAM]` → verify insights listed with evidence summaries
10. Try `psctl insights generate --scope team --team [TEAM]` → verify on-demand run completes

**After W2 (Launchpad Source):**
1. Open admin UI → Data Sources → click "Add Source" → select "Launchpad"
2. Fill in tracked projects; enter OAuth consumer key and token
3. Click "Test Connection" → verify success
4. Note the displayed webhook callback URL
5. Save the source → verify it appears in the source list
6. Trigger ingestion → check Ingestion Status page for Launchpad progress
7. Query `activity.contributions WHERE platform = 'launchpad'` → verify merge proposals and bug tasks landed
8. Check rich timestamp fields in `metadata` for a merge proposal (date_created, date_review_requested, etc.)
9. Check admin UI for unresolved Launchpad identities
10. If webhook configured: verify a Launchpad status change appears in `activity.webhook_events`

**After W3 (Mailing List Source):**
1. Open admin UI → Data Sources → click "Add Source" → select "Mailing List"
2. Fill in archive base URL, list names (e.g. `ubuntu-devel`), lookback months
3. Click "Test Connection" → verify archive URL is reachable
4. Save → trigger ingestion → check Ingestion Status page for mailing list progress
5. Query `activity.contributions WHERE platform = 'mailing_list'` → verify messages landed
6. Check threading: verify messages with same thread ID are grouped correctly
7. Check identity resolution: verify `From` email matched to `org.people` where possible
8. Verify unresolved mailing list identities appear in admin UI

**After W5 (Cross-Platform Correlation):**
1. Navigate to `/people/[personId]` → click "Timeline" tab
2. Verify unified chronological view shows activity across all platforms
3. Filter by platform (e.g. Launchpad only) → verify filtering works
4. Navigate to `/contributions/[contributionId]` for a Launchpad MP that references a GitHub PR
5. Verify "Cross-platform links" section shows the linked GitHub PR with link type and confidence
6. Click the linked contribution → verify navigation works
7. Open admin UI → Identity Suggestions → verify pending suggestions with accept/reject buttons
8. Accept a suggestion → verify the two accounts are now linked under one person
9. Navigate to `/insights` → verify cross-platform insights appear with `cross_platform` category badge
10. Try `psctl links [CONTRIBUTION_ID]` → verify cross-platform links displayed
11. Try `psctl identity-suggestions --review` → verify interactive review flow

### Cross-cutting

- **End-to-end navigation:** Verify the full path works with all Phase 4 additions: `/insights` → click insight → drill down to team/contribution → `/contributions/[contributionId]` (with cross-platform links) → `/people/[personId]` (with Timeline tab) → back via breadcrumbs.
- **Traceability audit:** Every insight on `/insights` must show evidence and reasoning trace. Every cross-platform link on `/contributions/[contributionId]` must show link type and confidence. No black boxes.
- **Identity resolution across 5+ platforms:** Verify admin UI surfaces unresolved identities from all sources (GitHub, Jira, Discourse, Launchpad, mailing lists). Verify identity suggestions work for cross-platform matches.
- **Backup/restore:** Create a backup after Phase 4 data is populated, restore to a fresh instance, verify all new data types (Launchpad contributions, mailing list messages, cross-platform links, insights, webhook events) are restored correctly.
