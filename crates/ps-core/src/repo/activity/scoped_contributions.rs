use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use super::ActivityRepo;
use super::scoped_changes::{read_contributions, record_changes};
use crate::{
    Error,
    ingestion::{ContributionInput, SourceRunContext},
    models::ContributionType,
};

impl ActivityRepo {
    /// Revalidate ownership and enqueue downstream work under the same locks
    /// as the upsert. Context from other actors is discarded before any writes.
    pub async fn store_person_contributions(
        &self,
        request: &SourceRunContext,
        run_id: Uuid,
        items: &[ContributionInput],
    ) -> Result<usize, Error> {
        request.validate()?;
        let prepared: Vec<_> = items.iter().map(with_enrichment_fingerprint).collect();
        let mut eligible = request.eligible_batch(&prepared)?;
        let mut tx = self.pool.begin().await?;
        validate_write_owner(&mut tx, request, run_id).await?;
        Self::lock_contribution_keys(&mut tx, &eligible).await?;

        let before = read_contributions(&mut tx, request, &eligible).await?;
        let person_id = request
            .scope
            .person_id()
            .ok_or_else(|| Error::Validation("person scope required".into()))?
            .into_inner();
        if before
            .values()
            .any(|row| row.person_id.is_some_and(|id| id != person_id))
        {
            return Err(Error::Conflict("existing contribution belongs to another person; repair attribution before retrying".into()));
        }
        for item in &eligible {
            if item.platform == crate::models::Platform::Jira
                && let Some(previous) = before.get(item.platform_id.as_str())
            {
                let old_origin = previous
                    .input
                    .get("url")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|url| reqwest::Url::parse(url).ok())
                    .map(|url| url.origin());
                let new_origin = item
                    .url
                    .as_deref()
                    .and_then(|url| reqwest::Url::parse(url).ok())
                    .map(|url| url.origin());
                if old_origin.is_none() || old_origin != new_origin {
                    return Err(Error::Conflict(
                        "Jira issue key already belongs to a different source instance".into(),
                    ));
                }
            }
        }

        // The feed is descending; a re-like of the same post uses the existing
        // natural key. Older pages must not reverse a newer authoritative event.
        eligible.retain(|item| {
            item.contribution_type != ContributionType::DiscourseLike
                || before
                    .get(item.platform_id.as_str())
                    .is_none_or(|previous| {
                        previous
                            .input
                            .get("metadata")
                            .and_then(|metadata| metadata.get("event_time_source"))
                            .and_then(serde_json::Value::as_str)
                            != Some("discourse_user_action")
                            || previous.created_at <= item.created_at
                    })
        });
        let ids: Vec<_> = eligible.iter().map(|_| Uuid::now_v7()).collect();
        let people = vec![Some(person_id); eligible.len()];
        let upserted =
            Self::bulk_upsert_in_transaction(&mut tx, &ids, &people, &eligible, true).await?;
        if upserted.len() != eligible.len() {
            return Err(Error::Conflict(
                "contribution ownership changed during ingestion".into(),
            ));
        }
        correct_like_timestamps(&mut tx, request, &eligible).await?;
        let after = read_contributions(&mut tx, request, &eligible).await?;
        let changed: Vec<_> = eligible
            .iter()
            .filter_map(|item| {
                let current = after.get(item.platform_id.as_str())?;
                let previous = before.get(item.platform_id.as_str());
                if previous.is_some_and(|previous| previous.input == current.input) {
                    return None;
                }
                Some((*item, current.id))
            })
            .collect();
        record_changes(&mut tx, request, run_id, &before, &after, &changed).await?;
        crate::repo::ReasoningRepo::enqueue_scoped_in_transaction(&mut tx, &changed).await?;
        tx.commit().await?;
        Ok(eligible.len())
    }
}

async fn validate_write_owner(
    tx: &mut Transaction<'_, Postgres>,
    request: &SourceRunContext,
    run_id: Uuid,
) -> Result<(), Error> {
    let owner = sqlx::query!(
        r#"
        SELECT (status IN ('pending', 'running') AND NOT cancellation_requested) AS "active!", request_snapshot
        FROM activity.pipelines WHERE id = $1 FOR UPDATE
        "#,
        request.pipeline_id,
    )
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Error::Conflict("person pipeline no longer exists".into()))?;
    let snapshot: crate::ingestion::PipelineRequest =
        serde_json::from_value(owner.request_snapshot)
            .map_err(|_| Error::Validation("pipeline has no valid admitted snapshot".into()))?;
    if snapshot.scope != request.scope
        || snapshot.since_date != request.since_date
        || snapshot.run_started_at != request.run_started_at
        || snapshot.processing != request.processing
        || !snapshot.sources.contains(&request.source)
    {
        return Err(Error::Validation(
            "source request does not match admitted pipeline snapshot".into(),
        ));
    }
    let source_name = &request.source.source_name;
    let owned_run = sqlx::query_scalar!(
        r#"
        SELECT id FROM activity.ingestion_runs
        WHERE id = $1 AND pipeline_id = $2 AND source_name = $3
            AND status = 'running' AND completed_at IS NULL
        FOR UPDATE
        "#,
        run_id,
        request.pipeline_id,
        source_name,
    )
    .fetch_optional(&mut **tx)
    .await?
    .is_some();
    if !owner.active || !owned_run {
        return Err(Error::Conflict(
            "person ingestion is cancelled or no longer owns its run".into(),
        ));
    }

    let identity = request
        .source
        .identity
        .as_ref()
        .ok_or_else(|| Error::Validation("saved identity required".into()))?;
    let person_id = identity.person_id.into_inner();
    let active_person = sqlx::query_scalar!(
        "SELECT active FROM org.people WHERE id = $1 FOR SHARE",
        person_id,
    )
    .fetch_optional(&mut **tx)
    .await?
    .unwrap_or(false);
    let saved_identity = sqlx::query!(
        r#"
        SELECT person_id, platform, platform_username, platform_user_id
        FROM org.platform_identities WHERE id = $1 FOR SHARE
        "#,
        identity.identity_id,
    )
    .fetch_optional(&mut **tx)
    .await?;
    let unchanged_identity = saved_identity.is_some_and(|saved| {
        saved.person_id == person_id
            && saved.platform == identity.platform.to_string()
            && saved.platform_username == identity.username.as_str()
            && saved.platform_user_id == identity.platform_user_id
    });
    if !active_person || !unchanged_identity {
        return Err(Error::Conflict(
            "person or saved identity changed; start a new backfill".into(),
        ));
    }

    let source = sqlx::query!(
        "SELECT name, source_type, enabled FROM config.source_configs WHERE id = $1 FOR SHARE",
        request.source.source_id.into_inner(),
    )
    .fetch_optional(&mut **tx)
    .await?;
    if source.is_none_or(|source| {
        !source.enabled
            || source.name != *source_name
            || source.source_type != request.source.platform.to_string()
    }) {
        return Err(Error::Conflict(
            "selected source changed or is disabled; start a new backfill".into(),
        ));
    }
    Ok(())
}

/// Only authoritative user-action evidence authorizes a `created_at` correction.
async fn correct_like_timestamps(
    tx: &mut Transaction<'_, Postgres>,
    request: &SourceRunContext,
    items: &[&ContributionInput],
) -> Result<(), Error> {
    if !request.source.platform.is_discourse() {
        return Ok(());
    }
    let mut keys = Vec::new();
    let mut dates: Vec<OffsetDateTime> = Vec::new();
    for item in items
        .iter()
        .filter(|item| item.contribution_type == ContributionType::DiscourseLike)
    {
        let event_time = item
            .metadata
            .get("event_created_at")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| {
                OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
            });
        let expected_key = format!(
            "like-{}-{}",
            item.metadata
                .get("post_id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_default(),
            item.platform_username.to_lowercase()
        );
        let expected_action_key = format!(
            "1:{}:{}:{}:{}",
            item.metadata
                .get("topic_id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_default(),
            item.metadata
                .get("post_id")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or_default(),
            item.platform_username.to_lowercase(),
            item.metadata
                .get("event_created_at")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
        );
        if item
            .metadata
            .get("event_time_source")
            .and_then(serde_json::Value::as_str)
            != Some("discourse_user_action")
            || item
                .metadata
                .get("user_action_type")
                .and_then(serde_json::Value::as_i64)
                != Some(1)
            || item
                .metadata
                .get("user_action_key")
                .and_then(serde_json::Value::as_str)
                != Some(expected_action_key.as_str())
            || event_time != Some(item.created_at)
            || item.platform_id.as_str() != expected_key
        {
            return Err(Error::Validation(
                "like timestamp correction requires matching Discourse user-action evidence".into(),
            ));
        }
        keys.push(item.platform_id.as_str());
        dates.push(item.created_at);
    }
    let platform = request.source.platform.to_string();
    let contribution_type = ContributionType::DiscourseLike.as_str();
    let person_id = request
        .scope
        .person_id()
        .map(crate::models::PersonId::into_inner);
    sqlx::query!(
        r#"
        UPDATE activity.contributions c SET created_at = input.created_at
        FROM UNNEST($1::text[], $2::timestamptz[]) AS input(platform_id, created_at)
        WHERE c.platform = $3 AND c.platform_id = input.platform_id
            AND c.contribution_type = $4 AND c.person_id = $5
        "#,
        &keys as &[&str],
        &dates,
        platform,
        contribution_type,
        person_id,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn with_enrichment_fingerprint(item: &ContributionInput) -> ContributionInput {
    let mut item = item.clone();
    if let Some(content) = &item.enrichment_content
        && let Some(metadata) = item.metadata.as_object_mut()
    {
        metadata.insert(
            "enrichment_input_hash".into(),
            crate::repo::reasoning::content_hash(content).into(),
        );
    }
    item
}
