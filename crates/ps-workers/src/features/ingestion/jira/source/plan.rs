use ps_core::ingestion::{IngestionContext, IngestionPlan};
use tracing::debug;

use super::DEFAULT_LOOKBACK_DAYS;

pub(super) async fn plan_impl(ctx: &IngestionContext) -> Result<IngestionPlan, ps_core::Error> {
    let settings = &ctx.source_config.settings;

    let projects = super::query::configured_projects(settings)?;
    if let Some(request) = ctx.person_request()? {
        super::query::validate_person_mode(ctx, request)?;
        let watermark = request
            .since_date
            .as_ref()
            .map(|date| format!("{date}T00:00:00Z"));
        return Ok(IngestionPlan {
            source_name: ctx.source_config.name.clone(),
            watermark,
            repos: vec![],
            items: projects,
        });
    }

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
        debug!(
            default_watermark = ?wm,
            "no watermark found — defaulting to {DEFAULT_LOOKBACK_DAYS}-day lookback"
        );
        wm
    });

    debug!(
        projects = ?projects,
        watermark = ?effective_watermark,
        "planned Jira ingestion"
    );

    Ok(IngestionPlan {
        source_name: ctx.source_config.name.clone(),
        watermark: effective_watermark,
        repos: vec![],
        items: projects,
    })
}
