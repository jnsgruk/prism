use ps_core::ingestion::{IngestionContext, IngestionPlan, RepoTarget};
use tracing::{debug, warn};

use super::super::repos;
use super::{DEFAULT_LOOKBACK_DAYS, build_rest_client, decrypt_token};

pub(super) async fn plan_impl(ctx: &IngestionContext) -> Result<IngestionPlan, ps_core::Error> {
    let settings = &ctx.source_config.settings;

    let orgs: Vec<String> = settings
        .get("orgs")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    if orgs.is_empty() {
        return Err(ps_core::Error::Validation(
            "GitHub source has no orgs configured".into(),
        ));
    }

    // Try to build repo list from team sync data (no API calls needed).
    let mapped_repos = ctx
        .repos
        .org
        .get_mapped_github_team_repos(ctx.source_config.id)
        .await?;

    let (final_repos, used_fallback) = if mapped_repos.is_empty() {
        // Fallback: no teams mapped yet, discover repos via REST (preserves
        // backwards compatibility for fresh setups before teams are configured).
        let exclude_archived = settings
            .get("exclude_archived")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let exclude_repos: Vec<String> = settings
            .get("exclude_repos")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let token = decrypt_token(ctx)?;
        let client = build_rest_client(ctx, &token);

        let discovered = repos::discover_repos(
            &client,
            &orgs,
            &ctx.repos.org,
            exclude_archived,
            &exclude_repos,
        )
        .await?;

        warn!(
            source = ctx.source_config.name,
            repos = discovered.len(),
            "no team mappings found — fell back to full org repo discovery"
        );

        (discovered, true)
    } else {
        let repos: Vec<RepoTarget> = mapped_repos
            .into_iter()
            .map(|(owner, repo)| RepoTarget { owner, repo })
            .collect();
        (repos, false)
    };

    // Load watermark. If none exists, default to 7 days ago.
    let watermark = ctx
        .repos
        .activity
        .get_watermark(&ctx.source_config.name)
        .await?;

    let effective_watermark = watermark.clone().or_else(|| {
        let seven_days_ago =
            time::OffsetDateTime::now_utc() - time::Duration::days(DEFAULT_LOOKBACK_DAYS);
        let wm = seven_days_ago
            .format(&time::format_description::well_known::Rfc3339)
            .ok();
        debug!(
            default_watermark = ?wm,
            "no watermark found — defaulting to {DEFAULT_LOOKBACK_DAYS}-day lookback"
        );
        wm
    });

    debug!(
        repos = final_repos.len(),
        watermark = ?effective_watermark,
        fallback_discovery = used_fallback,
        "planned GitHub ingestion"
    );

    Ok(IngestionPlan {
        source_name: ctx.source_config.name.clone(),
        watermark: effective_watermark,
        repos: final_repos,
        items: vec![],
    })
}
