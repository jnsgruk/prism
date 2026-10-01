//! Supplementary active-account discovery reuses the existing person adapters.
//! Source-wide cursors and account coverage have independent boundaries.
use async_trait::async_trait;
use ps_core::{
    Error,
    ingestion::{
        ContributionInput, FetchResult, IdentitySnapshot, IngestionContext, IngestionPlan,
        PipelineScope, ProcessingScope, SelectedSource, Source, SourceRunContext,
    },
    models::{Platform, WatermarkField},
};
use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use super::finalise::extract_failed_items;

const VERSION: u32 = 1;
const OVERLAP_DAYS: i64 = 1;

/// Registry wrapper: no scheduler, new provider API, or alternate storage path.
pub struct ActiveIdentitySource {
    inner: Box<dyn Source>,
}

impl ActiveIdentitySource {
    pub fn new(inner: Box<dyn Source>) -> Self {
        Self { inner }
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Target {
    request: SourceRunContext,
    version: String,
}

#[derive(Serialize, Deserialize)]
struct Cursor {
    active_identity_discovery: u32,
    cutoff: OffsetDateTime,
    child: String,
    global_done: bool,
    targets: Option<Vec<Target>>,
    target_index: usize,
    target_started: bool,
    /// One target completion, emitted only with the final stored target page.
    completed: Option<Target>,
    failed_items: Vec<ps_core::ingestion::FailedItem>,
    /// Flattened fields preserve source watermark/progress extraction.
    #[serde(flatten)]
    display: serde_json::Map<String, serde_json::Value>,
}

pub fn identity_version(ctx: &IngestionContext, identity: &IdentitySnapshot) -> String {
    ps_core::repo::reasoning::content_hash(&serde_json::json!({
        "identity": identity,
        "settings": ctx.source_config.settings,
    }))
}

fn lookback_days(platform: &Platform) -> i64 {
    match platform {
        Platform::Discourse(_) => 30,
        _ => 7,
    }
}

fn lower_date(
    cutoff: Option<OffsetDateTime>,
    upper: OffsetDateTime,
    platform: &Platform,
) -> String {
    let lower = cutoff.map_or_else(
        || upper - Duration::days(lookback_days(platform)),
        |cutoff| cutoff - Duration::days(OVERLAP_DAYS),
    );
    lower.date().to_string()
}

fn scoped_context(ctx: &IngestionContext, target: &Target) -> IngestionContext {
    let mut scoped = ctx.clone();
    // This snapshot is used only for fetch and event eligibility. Stores retain
    // the original All context and its actual run/pipeline ownership.
    scoped.request = Some(target.request.clone());
    scoped
}

impl ActiveIdentitySource {
    async fn targets(
        &self,
        ctx: &IngestionContext,
        upper: OffsetDateTime,
    ) -> Result<Vec<Target>, Error> {
        let identities = ctx
            .repos
            .org
            .active_discovery_identities(&ctx.source_config.source_type)
            .await?;
        let mut targets = Vec::with_capacity(identities.len());
        for identity in identities {
            let version = identity_version(ctx, &identity);
            let Some(window) = ctx
                .repos
                .activity
                .identity_discovery_window(
                    ctx.source_config.id.into_inner(),
                    identity.identity_id,
                    &version,
                )
                .await?
            else {
                // Accounts added after journalled planning join the next run.
                continue;
            };
            let since_date = window.covered_through.map_or_else(
                || window.initial_since.date().to_string(),
                |covered| lower_date(Some(covered), upper, &ctx.source_config.source_type),
            );
            let person_id = identity.person_id;
            targets.push(Target {
                request: SourceRunContext {
                    pipeline_id: Uuid::now_v7(),
                    scope: PipelineScope::Person { person_id },
                    source: SelectedSource {
                        source_id: ctx.source_config.id,
                        source_name: ctx.source_config.name.clone(),
                        platform: ctx.source_config.source_type.clone(),
                        identity: Some(identity),
                    },
                    since_date: Some(since_date),
                    run_started_at: upper,
                    processing: ProcessingScope::Person { person_id },
                },
                version,
            });
        }
        Ok(targets)
    }

    async fn fetch_tracked(
        &self,
        ctx: &IngestionContext,
        mut cursor: Cursor,
    ) -> Result<FetchResult, Error> {
        if cursor.active_identity_discovery != VERSION {
            return Err(Error::Validation(
                "unsupported activity discovery cursor".into(),
            ));
        }
        cursor.completed = None;
        let mut target = None;
        let mut fetch_context = ctx.clone();
        if cursor.global_done {
            if cursor.targets.is_none() {
                cursor.targets = Some(self.targets(ctx, cursor.cutoff).await?);
            }
            target = cursor
                .targets
                .as_ref()
                .and_then(|targets| targets.get(cursor.target_index))
                .cloned();
            let Some(current) = &target else {
                return cursor.result(vec![], None, true);
            };
            fetch_context = scoped_context(ctx, current);
            if !cursor.target_started {
                let plan = self.inner.plan(&fetch_context).await?;
                cursor.child = self.inner.initial_cursor(&fetch_context, &plan);
                cursor.target_started = true;
            }
        }
        let mut fetched = self
            .inner
            .fetch_batch(&fetch_context, &cursor.child)
            .await?;
        if let Some(target) = &target {
            let identity = serde_json::to_value(&target.request.source.identity)
                .map_err(|error| Error::Internal(error.to_string()))?;
            let source_id = serde_json::Value::from(target.request.source.source_id.to_string());
            for item in &mut fetched.items {
                if let Some(metadata) = item.metadata.as_object_mut() {
                    metadata.insert("supplementary_discovery_identity".into(), identity.clone());
                    metadata.insert(
                        "supplementary_discovery_source_id".into(),
                        source_id.clone(),
                    );
                }
            }
        }
        let state = fetched
            .etag
            .as_deref()
            .or(fetched.next_cursor.as_deref())
            .unwrap_or(&cursor.child);
        let child_failures = extract_failed_items(state);
        if target.is_none() {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(state) {
                for field in [
                    "completed_max_updated_at",
                    "max_updated_at",
                    "max_bumped_at",
                    "phase",
                    "repo_index",
                    "repos",
                    "page",
                ] {
                    if let Some(value) = value.get(field) {
                        cursor.display.insert(field.into(), value.clone());
                    }
                }
            }
        } else {
            cursor
                .display
                .insert("phase".into(), "ActiveIdentityDiscovery".into());
            cursor
                .display
                .insert("target_index".into(), cursor.target_index.into());
        }
        if let Some(next) = &fetched.next_cursor {
            cursor.child.clone_from(next);
        } else {
            let child_complete = child_failures.is_empty();
            cursor.failed_items.extend(child_failures);
            if let Some(current) = target {
                if child_complete {
                    cursor.completed = Some(current);
                }
                cursor.target_index += 1;
                cursor.target_started = false;
            } else {
                cursor.global_done = true;
            }
        }
        let done = cursor.global_done
            && cursor
                .targets
                .as_ref()
                .is_some_and(|targets| cursor.target_index >= targets.len());
        let raw =
            serde_json::to_string(&cursor).map_err(|error| Error::Internal(error.to_string()))?;
        fetched.next_cursor = (!done).then(|| raw.clone());
        fetched.etag = Some(raw);
        Ok(fetched)
    }
}

impl Cursor {
    fn result(
        self,
        items: Vec<ContributionInput>,
        rate_limit: Option<ps_core::models::RateLimitInfo>,
        done: bool,
    ) -> Result<FetchResult, Error> {
        let raw =
            serde_json::to_string(&self).map_err(|error| Error::Internal(error.to_string()))?;
        Ok(FetchResult {
            items,
            next_cursor: (!done).then(|| raw.clone()),
            etag: Some(raw),
            rate_limit,
            display_rate_limit: None,
            skipped_diffs: vec![],
        })
    }
}

#[async_trait]
impl Source for ActiveIdentitySource {
    fn name(&self) -> &'static str {
        self.inner.name()
    }
    fn supports_person_backfill(&self) -> bool {
        self.inner.supports_person_backfill()
    }
    fn watermark_field(&self) -> WatermarkField {
        self.inner.watermark_field()
    }
    async fn plan(&self, ctx: &IngestionContext) -> Result<IngestionPlan, Error> {
        let mut plan = self.inner.plan(ctx).await?;
        if ctx.person_request()?.is_none() {
            let cutoff = ctx
                .request
                .as_ref()
                .map_or_else(OffsetDateTime::now_utc, |request| request.run_started_at);
            // Plan is journalled by the coordinator. Never read the clock in
            // initial_cursor: replay must dispatch byte-identical child input.
            plan.discovery_cutoff = Some(cutoff);
            let identities = ctx
                .repos
                .org
                .active_discovery_identities(&ctx.source_config.source_type)
                .await?;
            let targets: Vec<_> = identities
                .into_iter()
                .map(|identity| {
                    let version = identity_version(ctx, &identity);
                    (identity, version)
                })
                .collect();
            let initial_since = ps_core::ingestion::parse_since_date(&lower_date(
                None,
                cutoff,
                &ctx.source_config.source_type,
            ))?
            .midnight()
            .assume_utc();
            ctx.repos
                .activity
                .begin_identity_discovery(
                    ctx.source_config.id.into_inner(),
                    &targets,
                    initial_since,
                )
                .await?;
        }
        Ok(plan)
    }
    fn initial_cursor(&self, ctx: &IngestionContext, plan: &IngestionPlan) -> String {
        let child = self.inner.initial_cursor(ctx, plan);
        if ctx.person_request().ok().flatten().is_some() {
            return child;
        }
        serde_json::to_string(&Cursor {
            active_identity_discovery: VERSION,
            cutoff: plan
                .discovery_cutoff
                .unwrap_or(ctx.source_config.updated_at),
            child,
            global_done: false,
            targets: None,
            target_index: 0,
            target_started: false,
            completed: None,
            failed_items: vec![],
            display: serde_json::Map::new(),
        })
        .unwrap_or_default()
    }
    async fn fetch_batch(&self, ctx: &IngestionContext, raw: &str) -> Result<FetchResult, Error> {
        if ctx.person_request()?.is_some() {
            return self.inner.fetch_batch(ctx, raw).await;
        }
        // Existing in-flight raw cursors retain their original traversal.
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|error| Error::Validation(error.to_string()))?;
        if value.get("active_identity_discovery").is_none() {
            return self.inner.fetch_batch(ctx, raw).await;
        }
        let cursor =
            serde_json::from_value(value).map_err(|error| Error::Validation(error.to_string()))?;
        self.fetch_tracked(ctx, cursor).await
    }
    async fn store_batch(
        &self,
        ctx: &IngestionContext,
        items: &[ContributionInput],
    ) -> Result<usize, Error> {
        // Scoped storage applies its own validated last-row deduplication.
        if ctx.person_request()?.is_some() {
            return self.inner.store_batch(ctx, items).await;
        }

        // Team/user overlap and shifted API pages can repeat natural keys.
        // Keep fetch indexes intact for deferred diffs; deduplicate only at the
        // persistence boundary before PostgreSQL's UNNEST upsert.
        let mut seen = std::collections::HashSet::new();
        let unique: Vec<_> = items
            .iter()
            .filter(|item| seen.insert(item.key()))
            .collect();
        if unique.len() == items.len() {
            return self.inner.store_batch(ctx, items).await;
        }
        let prepared: Vec<_> = unique.into_iter().cloned().collect();
        self.inner.store_batch(ctx, &prepared).await
    }
    async fn advance_watermark(
        &self,
        ctx: &IngestionContext,
        watermark: &str,
        items: i32,
    ) -> Result<(), Error> {
        self.inner.advance_watermark(ctx, watermark, items).await
    }
    async fn checkpoint_batch(&self, ctx: &IngestionContext, raw: &str) -> Result<(), Error> {
        let value: serde_json::Value =
            serde_json::from_str(raw).map_err(|error| Error::Validation(error.to_string()))?;
        let target = if value.get("active_identity_discovery").is_some() {
            let cursor: Cursor = serde_json::from_value(value)
                .map_err(|error| Error::Validation(error.to_string()))?;
            cursor.completed
        } else if value
            .get("discovery_complete")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
            && extract_failed_items(raw).is_empty()
        {
            ctx.person_request()?
                .map(|request| {
                    let identity = request
                        .source
                        .identity
                        .as_ref()
                        .ok_or_else(|| Error::Validation("saved identity required".into()))?;
                    Ok::<Target, Error>(Target {
                        request: request.clone(),
                        version: identity_version(ctx, identity),
                    })
                })
                .transpose()?
        } else {
            None
        };
        if let Some(target) = target {
            ctx.repos
                .activity
                .advance_identity_discovery(
                    target.request.source.source_id.into_inner(),
                    target
                        .request
                        .source
                        .identity
                        .as_ref()
                        .ok_or_else(|| Error::Validation("missing discovery identity".into()))?,
                    &target.version,
                    target.request.run_started_at,
                )
                .await?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn first_discovery_ignores_global_watermark_and_incremental_boundary_overlaps() {
        let upper = OffsetDateTime::from_unix_timestamp(1_800_000_000).unwrap();
        assert_eq!(
            lower_date(None, upper, &Platform::Github),
            (upper - Duration::days(7)).date().to_string()
        );
        assert_eq!(
            lower_date(None, upper, &Platform::Discourse("ubuntu".into())),
            (upper - Duration::days(30)).date().to_string()
        );
        assert_eq!(
            lower_date(Some(upper), upper, &Platform::Github),
            (upper - Duration::days(1)).date().to_string()
        );
    }
}
