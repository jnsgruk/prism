mod grpc;
mod restate;

use std::sync::LazyLock;

use ps_core::repo::Repos;
use ps_proto::prism::v1::{HandlerInfo, SourceState};
use regex::Regex;
use tonic::Status;

/// Single source of truth for all known handler/method combinations.
///
/// Each tuple is `(handler_name, &[methods], description, requires_key)`.
pub(crate) const HANDLER_DEFS: &[(&str, &[&str], &str, bool)] = &[
    (
        "GithubIngestionHandler",
        &["run_ingestion", "backfill"],
        "Fetches pull requests and reviews from GitHub repositories",
        true,
    ),
    (
        "JiraIngestionHandler",
        &["run_ingestion", "backfill"],
        "Fetches issues, changelogs, and status transitions from Jira",
        true,
    ),
    (
        "DiscourseIngestionHandler",
        &["run_ingestion", "backfill"],
        "Fetches topics and posts from a Discourse instance",
        true,
    ),
    (
        "GithubTeamSyncHandler",
        &["sync_teams"],
        "Discovers GitHub teams, members, and repos for configured orgs",
        true,
    ),
    (
        "IdentityResolutionHandler",
        &["resolve_identities"],
        "Resolves pending platform identities for known directory people across all Discourse sources",
        false,
    ),
    (
        "MetricsComputeHandler",
        &["compute_current_periods"],
        "Recomputes metric snapshots for all teams across current week/month/quarter",
        false,
    ),
    (
        "EnrichmentHandler",
        &["run_cycle"],
        "AI enrichment pipeline — scores review depth, sentiment, PR significance, and topic classification",
        false,
    ),
    (
        "InsightsHandler",
        &["compute_current_periods"],
        "Recomputes insight snapshots from enrichment data for all teams across current periods",
        false,
    ),
    (
        "EmbeddingHandler",
        &["run_cycle"],
        "Generates vector embeddings for contributions using the configured AI provider",
        false,
    ),
    (
        "ModelCatalogueHandler",
        &["refresh_catalogue"],
        "Fetches available models from configured AI providers and caches them locally",
        false,
    ),
];

/// Map a platform to its Restate ingestion handler name.
#[allow(clippy::result_large_err)]
pub(crate) fn handler_for_platform(
    platform: &ps_core::models::Platform,
) -> Result<&'static str, Status> {
    match platform {
        ps_core::models::Platform::Github => Ok("GithubIngestionHandler"),
        ps_core::models::Platform::Jira => Ok("JiraIngestionHandler"),
        ps_core::models::Platform::Discourse(_) => Ok("DiscourseIngestionHandler"),
        _ => Err(Status::unimplemented(format!(
            "no ingestion handler for platform: {platform}"
        ))),
    }
}

/// Build the list of `HandlerInfo` proto messages from the static definitions.
pub(crate) fn known_handlers() -> Vec<HandlerInfo> {
    HANDLER_DEFS
        .iter()
        .map(|(name, methods, description, requires_key)| HandlerInfo {
            name: (*name).into(),
            methods: methods.iter().map(|m| (*m).to_string()).collect(),
            description: (*description).into(),
            requires_key: *requires_key,
            active_run: None,
        })
        .collect()
}

/// Only allow safe identifiers in Restate SQL queries (no parameterised query support).
static SAFE_IDENTIFIER: LazyLock<Regex> = LazyLock::new(|| {
    // SAFETY: This is a compile-time-valid regex pattern
    #[allow(clippy::expect_used)]
    Regex::new(r"^[a-zA-Z0-9_.:-]+$").expect("valid regex")
});

#[allow(clippy::result_large_err)]
pub(crate) fn validate_restate_identifier(s: &str) -> Result<&str, Status> {
    if s.is_empty() || !SAFE_IDENTIFIER.is_match(s) {
        return Err(Status::invalid_argument(format!(
            "invalid identifier for Restate query: {s:?}"
        )));
    }
    Ok(s)
}

/// Derive the current source state from run data and watermarks.
pub(crate) fn derive_state(
    has_active_run: bool,
    last_successful_run: Option<time::OffsetDateTime>,
    last_error: Option<&str>,
) -> SourceState {
    if has_active_run {
        return SourceState::Collecting;
    }
    match (last_successful_run, last_error) {
        (_, Some(_)) => SourceState::Error,
        (Some(_), None) => SourceState::Idle,
        (None, None) => SourceState::Waiting,
    }
}

pub struct HandlersServiceImpl {
    pub(crate) repos: Repos,
    pub(crate) restate_url: String,
    pub(crate) restate_admin_url: String,
    pub(crate) http_client: reqwest::Client,
}

impl HandlersServiceImpl {
    pub fn new(repos: Repos, restate_url: String, restate_admin_url: String) -> Self {
        Self {
            repos,
            restate_url,
            restate_admin_url,
            http_client: reqwest::Client::new(),
        }
    }
}
