use std::collections::HashMap;

use sqlx::PgPool;
use uuid::Uuid;

/// Look up person IDs for a batch of GitHub usernames.
///
/// Returns a map from GitHub username → `person_id` for any usernames that
/// have a matching entry in `org.platform_identities`.
pub async fn batch_resolve_person_ids(
    pool: &PgPool,
    usernames: &[String],
) -> Result<HashMap<String, Uuid>, sqlx::Error> {
    if usernames.is_empty() {
        return Ok(HashMap::new());
    }

    let rows = sqlx::query!(
        r#"
        SELECT platform_username, person_id
        FROM org.platform_identities
        WHERE platform = 'github'
          AND platform_username = ANY($1)
        "#,
        usernames,
    )
    .fetch_all(pool)
    .await?;

    let map = rows
        .into_iter()
        .map(|r| (r.platform_username, r.person_id))
        .collect();
    Ok(map)
}
