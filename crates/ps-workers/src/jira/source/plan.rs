use ps_core::ingestion::{IngestionContext, IngestionPlan};
use tracing::info;

use super::DEFAULT_LOOKBACK_DAYS;

pub(super) async fn plan_impl(ctx: &IngestionContext) -> Result<IngestionPlan, ps_core::Error> {
    let settings = &ctx.source_config.settings;

    let projects: Vec<String> = settings
        .get("projects")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // Load watermark. If none exists, default to 30 days ago.
    let watermark = ctx
        .repos
        .activity
        .get_watermark(&ctx.source_config.name)
        .await?
        .filter(|w| !w.is_empty());

    let effective_watermark = watermark.clone().or_else(|| {
        let lookback =
            time::OffsetDateTime::now_utc() - time::Duration::days(DEFAULT_LOOKBACK_DAYS);
        let wm = lookback
            .format(&time::format_description::well_known::Rfc3339)
            .ok();
        info!(
            source = ctx.source_config.name,
            default_watermark = ?wm,
            "no watermark found — defaulting to {DEFAULT_LOOKBACK_DAYS}-day lookback"
        );
        wm
    });

    info!(
        source = ctx.source_config.name,
        projects = ?projects,
        watermark = ?effective_watermark,
        "planned Jira ingestion"
    );

    Ok(IngestionPlan {
        source_name: ctx.source_config.name.clone(),
        watermark: effective_watermark,
        // Jira doesn't use the repos field — projects are tracked in the cursor.
        repos: vec![],
    })
}
