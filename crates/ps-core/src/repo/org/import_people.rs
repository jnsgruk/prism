use super::{ImportRecord, import::ImportState};
use crate::{
    Error,
    models::{Management, Platform},
};
use sqlx::PgConnection;
use uuid::Uuid;

/// Stable directory IDs win. Email is used only when exactly one compatible
/// person matches; ambiguous rows are left untouched for an administrator.
pub(super) async fn upsert_person(
    tx: &mut PgConnection,
    record: &ImportRecord,
    state: &mut ImportState,
) -> Result<Option<Uuid>, Error> {
    if let Some(dir_id) = &record.directory_id {
        let existing = sqlx::query_scalar!(
            "SELECT id FROM org.people WHERE directory_id = $1 FOR UPDATE",
            dir_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(Error::from)?;
        if let Some(id) = existing {
            return update_existing(tx, record, id, state).await.map(Some);
        }
    }
    if let Some(email) = record
        .email
        .as_deref()
        .map(str::trim)
        .filter(|e| !e.is_empty())
    {
        let candidates = sqlx::query!(
            r#"
            SELECT id, directory_id
            FROM org.people
            WHERE lower(btrim(email)) = lower($1)
            FOR UPDATE
            "#,
            email,
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(Error::from)?;
        if candidates.len() > 1
            || candidates.first().is_some_and(|c| {
                record.directory_id.is_some()
                    && c.directory_id.is_some()
                    && c.directory_id != record.directory_id
            })
        {
            state.warnings.push(format!(
                "Ambiguous or conflicting email match for {} — skipped; repair directory mapping",
                record.name
            ));
            return Ok(None);
        }
        if let Some(candidate) = candidates.first() {
            return update_existing(tx, record, candidate.id, state)
                .await
                .map(Some);
        }
    }

    if has_unproven_manual_match(tx, record).await? {
        state.warnings.push(format!(
            "Name or account match for {} lacks a stable directory ID or unique email — skipped; repair directory mapping to the manually managed person",
            record.name
        ));
        return Ok(None);
    }

    let id = Uuid::now_v7();
    sqlx::query!(
        r#"
        INSERT INTO org.people (id, name, email, level, directory_id, last_import_at)
        VALUES ($1, $2, $3, $4, $5, now())
        "#,
        id,
        record.name,
        record.email,
        record.level,
        record.directory_id,
    )
    .execute(&mut *tx)
    .await
    .map_err(Error::from)?;

    state.people_imported += 1;
    Ok(Some(id))
}

/// A name or account can flag an existing manual person, but cannot prove that
/// an incoming directory row belongs to that person. Require explicit repair.
async fn has_unproven_manual_match(
    tx: &mut PgConnection,
    record: &ImportRecord,
) -> Result<bool, Error> {
    let (platforms, usernames): (Vec<String>, Vec<String>) = record
        .identities
        .iter()
        .filter_map(|identity| {
            let platform = identity.platform.parse::<Platform>().ok()?;
            let username = identity.username.trim().to_lowercase();
            (!username.is_empty()).then(|| (platform.to_string(), username))
        })
        .unzip();
    let manual = Management::Manual;
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
                    SELECT 1
                    FROM org.people p
                    WHERE p.membership_management = $1
                      AND lower(btrim(p.name)) = lower($2)
                    UNION ALL
                    SELECT 1
                    FROM org.platform_identities pi
                    JOIN UNNEST($3::text[], $4::text[]) AS incoming(platform, username)
                      ON pi.platform = incoming.platform
         AND lower(pi.platform_username) = incoming.username
                    WHERE pi.management = $1
                ) AS "needs_repair!"
        "#,
        manual as Management,
        record.name.trim(),
        &platforms,
        &usernames,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(Error::from)
}

async fn update_existing(
    tx: &mut PgConnection,
    record: &ImportRecord,
    id: Uuid,
    state: &mut ImportState,
) -> Result<Uuid, Error> {
    sqlx::query!(
        r#"
        UPDATE org.people
        SET name = $1,
            email = $2,
            level = $3,
            directory_id = COALESCE(directory_id, $4),
            last_import_at = now(),
            updated_at = now()
        WHERE id = $5
        "#,
        record.name,
        record.email,
        record.level,
        record.directory_id,
        id,
    )
    .execute(&mut *tx)
    .await
    .map_err(Error::from)?;

    state.people_updated += 1;
    Ok(id)
}

pub(super) async fn map_identities(
    tx: &mut PgConnection,
    record: &ImportRecord,
    person_id: Uuid,
    state: &mut ImportState,
) -> Result<(), Error> {
    let mut ids = Vec::new();
    let mut platforms = Vec::new();
    let mut usernames = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for identity in &record.identities {
        let Ok(platform) = identity.platform.parse::<Platform>() else {
            state.warnings.push(format!(
                "Invalid platform for {} — identity skipped",
                record.name
            ));
            continue;
        };
        let username = identity.username.trim().to_lowercase();
        if username.is_empty() {
            state
                .warnings
                .push(format!("Empty identity for {} — skipped", record.name));
            continue;
        }
        let platform = platform.to_string();
        if seen.insert((platform.clone(), username.clone())) {
            ids.push(Uuid::now_v7());
            platforms.push(platform);
            usernames.push(username);
        }
    }
    if ids.is_empty() {
        return Ok(());
    }
    let person_ids = vec![person_id; ids.len()];
    let result = sqlx::query_scalar!(
        r#"
        INSERT INTO org.platform_identities (id, person_id, platform, platform_username)
        SELECT i.id, i.person_id, i.platform, i.username
        FROM UNNEST($1::uuid[], $2::uuid[], $3::text[], $4::text[])
            AS i(id, person_id, platform, username)
        WHERE NOT EXISTS (
            SELECT 1
            FROM org.identity_resolutions ir
            WHERE ir.person_id = i.person_id
              AND ir.platform = i.platform
              AND ir.status = 'manual'
        )
        ON CONFLICT (platform, platform_username)
            WHERE platform <> 'jira' OR platform_user_id IS NULL
        DO UPDATE
        SET person_id = EXCLUDED.person_id
        WHERE org.platform_identities.person_id = EXCLUDED.person_id
        RETURNING id
        "#,
        &ids,
        &person_ids,
        &platforms,
        &usernames,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(Error::from)?;
    if result.len() != ids.len() {
        state.warnings.push(format!(
            "Manually owned account conflict for {} — protected accounts skipped",
            record.name
        ));
    }

    state.identities_mapped += i32::try_from(result.len()).unwrap_or(i32::MAX);

    Ok(())
}
