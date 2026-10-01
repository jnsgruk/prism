use sqlx::{Postgres, Transaction};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{Error, ingestion::ContributionInput, models::ContributionType};

/// Only authoritative user-action evidence authorizes a `created_at` correction.
pub(super) async fn correct_like_timestamps(
    tx: &mut Transaction<'_, Postgres>,
    platform: &crate::models::Platform,
    person_id: Option<Uuid>,
    items: &[&ContributionInput],
) -> Result<(), Error> {
    if !platform.is_discourse() {
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
    let platform = platform.to_string();
    let contribution_type = ContributionType::DiscourseLike.as_str();
    sqlx::query!(
        r#"
        UPDATE activity.contributions c SET created_at = input.created_at
        FROM UNNEST($1::text[], $2::timestamptz[]) AS input(platform_id, created_at)
        WHERE c.platform = $3 AND c.platform_id = input.platform_id
            AND c.contribution_type = $4 AND ($5::uuid IS NULL OR c.person_id = $5)
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
