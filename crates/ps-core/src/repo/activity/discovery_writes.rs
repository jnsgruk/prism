//! Frozen supplementary targets are validated under the contribution transaction.
use std::collections::BTreeMap;

use sqlx::{Postgres, Transaction};
use uuid::Uuid;

use crate::{
    Error,
    ingestion::{ContributionInput, IdentitySnapshot},
};

pub(super) async fn validate_supplementary_targets(
    tx: &mut Transaction<'_, Postgres>,
    person_ids: &[Option<Uuid>],
    items: &[&ContributionInput],
) -> Result<(), Error> {
    let mut targets = BTreeMap::new();
    for (item, person_id) in items.iter().zip(person_ids) {
        let Some(value) = item.metadata.get("supplementary_discovery_identity") else {
            continue;
        };
        let identity: IdentitySnapshot = serde_json::from_value(value.clone())
            .map_err(|_| Error::Validation("invalid discovery identity".into()))?;
        let source_id = item
            .metadata
            .get("supplementary_discovery_source_id")
            .and_then(serde_json::Value::as_str)
            .and_then(|value| Uuid::parse_str(value).ok())
            .ok_or_else(|| Error::Validation("discovery source is required".into()))?;
        if *person_id != Some(identity.person_id.into_inner())
            || item.platform != identity.platform
            || !item
                .platform_username
                .eq_ignore_ascii_case(identity.username.as_str())
        {
            return Err(Error::Conflict(
                "discovery identity attribution changed; start a new ingestion run".into(),
            ));
        }
        targets.insert((identity.identity_id, source_id), identity);
    }
    for ((identity_id, source_id), identity) in targets {
        // Match scoped storage's person -> identity -> source -> contribution
        // lock order. Editing/deactivation cannot cross this write boundary.
        let active = sqlx::query_scalar!(
            "SELECT active FROM org.people WHERE id = $1 FOR SHARE",
            identity.person_id.into_inner(),
        )
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or(false);
        let saved = sqlx::query!(
            r#"
            SELECT person_id, platform, platform_username, platform_user_id
            FROM org.platform_identities WHERE id = $1 FOR SHARE
            "#,
            identity_id,
        )
        .fetch_optional(&mut **tx)
        .await?;
        let unchanged = saved.is_some_and(|saved| {
            saved.person_id == identity.person_id.into_inner()
                && saved.platform == identity.platform.to_string()
                && saved.platform_username == identity.username.as_str()
                && saved.platform_user_id == identity.platform_user_id
        });
        let source = sqlx::query!(
            "SELECT source_type, enabled FROM config.source_configs WHERE id = $1 FOR SHARE",
            source_id,
        )
        .fetch_optional(&mut **tx)
        .await?;
        if !active
            || !unchanged
            || source.is_none_or(|source| {
                !source.enabled || source.source_type != identity.platform.to_string()
            })
        {
            return Err(Error::Conflict(
                "saved discovery target changed; start a new ingestion run".into(),
            ));
        }
    }
    Ok(())
}
