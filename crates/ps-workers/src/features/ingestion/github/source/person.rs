//! Person discovery partitions on immutable PR creation time. Reviews may belong
//! to PRs older than the event window, so discovery starts at GitHub's inception.
use ps_core::ingestion::{
    ContributionInput, FetchResult, IngestionContext, IngestionPlan, SourceRunContext,
};
use ps_core::models::Platform;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use super::super::graphql::GraphQLClientError;
use super::{Cursor, build_graphql_client, decrypt_token, is_valid_github_username};
use crate::infra::retry::retry_transient;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum SearchPhase {
    Author,
    Reviewer,
}
impl SearchPhase {
    fn qualifier(&self) -> &'static str {
        match self {
            Self::Author => "author",
            Self::Reviewer => "reviewed-by",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Partition {
    pub start: i64,
    pub end: i64,
}
impl Partition {
    fn split(&self) -> Option<(Self, Self)> {
        if self.end <= self.start {
            return None;
        }
        let midpoint = self.start + (self.end - self.start) / 2;
        Some((
            Self {
                start: self.start,
                end: midpoint,
            },
            Self {
                start: midpoint + 1,
                end: self.end,
            },
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PersonCursor {
    #[serde(default = "cursor_version")]
    pub version: u32,
    #[serde(default)]
    pub request: Option<SourceRunContext>,
    #[serde(default)]
    pub base_url: String,
    pub username: String,
    pub orgs: Vec<String>,
    pub exclude_repos: Vec<String>,
    pub exclude_archived: bool,
    pub phase: SearchPhase,
    pub org_index: usize,
    pub partitions: Vec<Partition>,
    pub page: Option<String>,
    pub end: i64,
}
fn cursor_version() -> u32 {
    1
}
// GitHub launched in 2008. This conservative epoch covers imported old PRs too.
const EARLIEST: i64 = 0;

fn settings_strings(ctx: &IngestionContext, name: &str) -> Result<Vec<String>, ps_core::Error> {
    ctx.source_config
        .settings
        .get(name)
        .map_or(Ok(vec![]), |value| {
            serde_json::from_value(value.clone())
                .map_err(|_| ps_core::Error::Validation(format!("invalid GitHub {name}")))
        })
}

fn configured_base_url(ctx: &IngestionContext) -> &str {
    ctx.source_config
        .settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("https://api.github.com")
}

pub(super) fn validate(ctx: &IngestionContext) -> Result<bool, ps_core::Error> {
    let Some(request) = ctx.person_request()? else {
        return Ok(false);
    };
    request.validate()?;
    let endpoint = reqwest::Url::parse(configured_base_url(ctx))
        .map_err(|_| ps_core::Error::Validation("GitHub source endpoint is invalid".into()))?;
    if !matches!(endpoint.scheme(), "http" | "https")
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
    {
        return Err(ps_core::Error::Validation(
            "GitHub source endpoint must use HTTP(S) without credentials, a query, or a fragment"
                .into(),
        ));
    }
    let identity =
        request.source.identity.as_ref().ok_or_else(|| {
            ps_core::Error::Validation("GitHub person identity is required".into())
        })?;
    if identity.platform != Platform::Github || !is_valid_github_username(&identity.username) {
        return Err(ps_core::Error::Validation(
            "invalid GitHub person identity".into(),
        ));
    }
    let orgs = settings_strings(ctx, "orgs")?;
    if orgs.is_empty() || orgs.iter().any(|org| !is_valid_github_username(org)) {
        return Err(ps_core::Error::Validation(
            "GitHub requires valid configured organisations".into(),
        ));
    }
    if settings_strings(ctx, "exclude_repos")?.iter().any(|repo| {
        let parts: Vec<_> = repo.split('/').collect();
        match parts.as_slice() {
            [name] => !super::repositories::valid_name(name),
            [owner, name] => {
                !is_valid_github_username(owner) || !super::repositories::valid_name(name)
            }
            _ => true,
        }
    }) {
        return Err(ps_core::Error::Validation(
            "invalid GitHub repository exclusion".into(),
        ));
    }
    Ok(true)
}

pub(super) fn initial_person_cursor(ctx: &IngestionContext) -> Option<PersonCursor> {
    if !validate(ctx).ok()? {
        return None;
    }
    let request = ctx
        .request
        .as_ref()
        .filter(|request| request.scope.person_id().is_some())?;
    Some(PersonCursor {
        version: 1,
        request: Some(request.clone()),
        base_url: configured_base_url(ctx).into(),
        username: request.source.identity.as_ref()?.username.to_string(),
        orgs: settings_strings(ctx, "orgs").ok()?,
        exclude_repos: settings_strings(ctx, "exclude_repos").ok()?,
        exclude_archived: ctx
            .source_config
            .settings
            .get("exclude_archived")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true),
        phase: SearchPhase::Author,
        org_index: 0,
        partitions: vec![Partition {
            start: EARLIEST,
            end: request.run_started_at.unix_timestamp(),
        }],
        page: None,
        end: request.run_started_at.unix_timestamp(),
    })
}

pub(super) fn plan(ctx: &IngestionContext) -> IngestionPlan {
    IngestionPlan {
        discovery_cutoff: None,
        source_name: ctx.source_config.name.clone(),
        watermark: None,
        repos: vec![],
        items: vec![],
    }
}

pub(super) fn eligible(
    ctx: &IngestionContext,
    item: &ContributionInput,
) -> Result<bool, ps_core::Error> {
    let Some(request) = ctx.person_request()? else {
        return Ok(true);
    };
    request.eligible_contribution(item)
}

fn timestamp(seconds: i64) -> Result<String, ps_core::Error> {
    OffsetDateTime::from_unix_timestamp(seconds)
        .map_err(|error| ps_core::Error::Internal(error.to_string()))?
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|error| ps_core::Error::Internal(error.to_string()))
}
fn query(
    person: &PersonCursor,
    org: &str,
    partition: &Partition,
) -> Result<String, ps_core::Error> {
    if !is_valid_github_username(&person.username) || !is_valid_github_username(org) {
        return Err(ps_core::Error::Validation(
            "invalid GitHub search identifier".into(),
        ));
    }
    Ok(format!(
        "type:pr org:{org} {}:{} created:{}..{} sort:created-asc",
        person.phase.qualifier(),
        person.username,
        timestamp(partition.start)?,
        timestamp(partition.end)?
    ))
}

pub(super) fn validate_cursor(
    ctx: &IngestionContext,
    person: &PersonCursor,
) -> Result<(), ps_core::Error> {
    if person.request.as_ref() != ctx.person_request()? || person.request.is_none() {
        return Err(ps_core::Error::Validation(
            "GitHub cursor belongs to a different or missing frozen run; start a new backfill"
                .into(),
        ));
    }
    if person.base_url != configured_base_url(ctx) {
        return Err(ps_core::Error::Validation(
            "GitHub source endpoint changed; start a new backfill".into(),
        ));
    }
    if ctx
        .person_request()?
        .and_then(|request| request.source.identity.as_ref())
        .is_none_or(|identity| !person.username.eq_ignore_ascii_case(&identity.username))
    {
        return Err(ps_core::Error::Validation(
            "GitHub cursor identity does not match the frozen target".into(),
        ));
    }
    if person.version != 1 {
        return Err(ps_core::Error::Validation(
            "unsupported GitHub person cursor version".into(),
        ));
    }
    Ok(())
}

pub(super) async fn fetch(
    ctx: &IngestionContext,
    cur: &mut Cursor,
) -> Result<FetchResult, ps_core::Error> {
    validate(ctx)?;
    let person = cur
        .person
        .as_mut()
        .ok_or_else(|| ps_core::Error::Validation("missing GitHub person cursor".into()))?;
    validate_cursor(ctx, person)?;
    if person.partitions.is_empty() {
        person.org_index += 1;
        person.page = None;
        person.partitions.push(Partition {
            start: EARLIEST,
            end: person.end,
        });
    }
    if person.org_index >= person.orgs.len() {
        match person.phase {
            SearchPhase::Author => {
                person.phase = SearchPhase::Reviewer;
                person.org_index = 0;
            }
            SearchPhase::Reviewer => {
                return Ok(FetchResult {
                    items: vec![],
                    next_cursor: None,
                    rate_limit: None,
                    display_rate_limit: None,
                    etag: None,
                    skipped_diffs: vec![],
                });
            }
        }
    }
    let org = person
        .orgs
        .get(person.org_index)
        .cloned()
        .ok_or_else(|| ps_core::Error::Validation("missing person organisation".into()))?;
    let partition = person
        .partitions
        .last()
        .cloned()
        .ok_or_else(|| ps_core::Error::Internal("missing search partition".into()))?;
    let search = query(person, &org, &partition)?;
    let token = decrypt_token(ctx)?;
    let client = build_graphql_client(ctx, &token);
    let page = match retry_transient(
        "GitHub person search",
        GraphQLClientError::is_transient,
        || client.search_pull_requests(&search, person.page.as_deref()),
    )
    .await
    {
        Ok(page) => page,
        Err(error) => {
            if let Some(limit) = super::reviews::rate_limited(&error) {
                return super::reviews::result(cur, vec![], Some(limit));
            }
            if error.is_transient() {
                return Err(ps_core::Error::Internal(format!(
                    "GitHub person search failed: {error}"
                )));
            }
            cur.failed_items.push(ps_core::ingestion::FailedItem {
                key: search,
                error: super::reviews::person_error(&error),
            });
            person.partitions.pop();
            person.page = None;
            return super::reviews::result(cur, vec![], None);
        }
    };

    // Probe counts before accepting a capped search. Depth-first subdivision
    // keeps the pending partition stack logarithmic in the history window.
    if page.issue_count.is_none_or(|count| count >= 1000) {
        person.page = None;
        person.partitions.pop();
        if let Some((left, right)) = partition.split().filter(|_| page.issue_count.is_some()) {
            person.partitions.push(right);
            person.partitions.push(left);
        } else {
            cur.failed_items.push(ps_core::ingestion::FailedItem { key: search, error: "incomplete GitHub coverage: minimum creation-time partition exceeds search cap or omits count".into() });
        }
        return super::reviews::result(cur, vec![], Some(page.rate_limit));
    }
    if page.has_next_page {
        person.page = Some(
            page.end_cursor
                .clone()
                .filter(|cursor| Some(cursor) != person.page.as_ref())
                .ok_or_else(|| {
                    ps_core::Error::Internal("GitHub search did not advance pagination".into())
                })?,
        );
    } else {
        person.partitions.pop();
        person.page = None;
    }

    let bounds = person.clone();
    let mut items = Vec::new();
    for pr in &page.items {
        let Some(repo) = &pr.repository else {
            cur.failed_items.push(ps_core::ingestion::FailedItem {
                key: search.clone(),
                error: "incomplete GitHub coverage: malformed PR search node".into(),
            });
            continue;
        };
        if pr.number.is_none() || pr.created_at.is_none() || pr.updated_at.is_none() {
            cur.failed_items.push(ps_core::ingestion::FailedItem {
                key: search.clone(),
                error: "incomplete GitHub coverage: malformed PR search fields".into(),
            });
            continue;
        }
        let owner = &repo.owner.login;
        let name = &repo.name;
        match super::repositories::eligible(
            repo,
            std::slice::from_ref(&org),
            &bounds.exclude_repos,
            bounds.exclude_archived,
        ) {
            Ok(true) => {}
            Ok(false) => continue,
            Err(error) => {
                cur.failed_items.push(ps_core::ingestion::FailedItem {
                    key: format!("{owner}/{name}"),
                    error: error.to_string(),
                });
                continue;
            }
        }
        for item in super::convert::search_pr_to_contributions(owner, name, pr)? {
            if eligible(ctx, &item)? {
                items.push(item);
            }
        }
        super::reviews::queue(cur, owner, name, pr)?;
    }
    let diff = super::diff::fetch_pr_diffs(ctx, &mut items).await;
    let mut result = super::reviews::result(cur, items, diff.rate_limit.or(Some(page.rate_limit)))?;
    result.skipped_diffs = diff.skipped;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partitions_cover_boundary_without_gaps_or_overlap_and_terminate() {
        for end in [1, 2, 99, 1_000_000] {
            let partition = Partition { start: 0, end };
            let (left, right) = partition.split().unwrap();
            assert_eq!(left.start, partition.start);
            assert_eq!(right.end, partition.end);
            assert_eq!(left.end + 1, right.start);
            let mut window = partition;
            for _ in 0..32 {
                if let Some((left, _)) = window.split() {
                    window = left;
                } else {
                    break;
                }
            }
            assert_eq!(window.start, window.end);
            assert!(window.split().is_none());
        }
    }

    #[test]
    fn query_is_safe_and_review_discovery_has_no_mutable_upper_bound() {
        let mut cursor = PersonCursor {
            version: 1,
            request: None,
            base_url: "https://api.github.com".into(),
            username: "alice".into(),
            orgs: vec!["org".into()],
            exclude_repos: vec![],
            exclude_archived: true,
            phase: SearchPhase::Reviewer,
            org_index: 0,
            partitions: vec![],
            page: None,
            end: 1,
        };
        let query_string = query(&cursor, "org", &Partition { start: 0, end: 1 }).unwrap();
        assert!(query_string.contains(
            "org:org reviewed-by:alice created:1970-01-01T00:00:00Z..1970-01-01T00:00:01Z"
        ));
        assert!(!query_string.contains("updated:"));
        cursor.username = "alice org:outside".into();
        assert!(query(&cursor, "org", &Partition { start: 0, end: 1 }).is_err());
    }

    #[test]
    fn person_cursor_defaults_version() {
        let value = serde_json::json!({"username":"alice", "orgs":["org"], "exclude_repos": [], "exclude_archived": true, "phase":"Author", "org_index":0, "partitions":[], "page":null, "end":1});
        let cursor: PersonCursor = serde_json::from_value(value).unwrap();
        assert_eq!(cursor.version, 1);
    }
}
