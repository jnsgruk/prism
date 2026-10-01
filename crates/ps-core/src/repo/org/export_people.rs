//! Person reconciliation and protected account restoration for portable org exports.

use super::export::{ExportPerson, OrgExport, OrgImportResult};
use crate::{
    Error,
    models::{Management, Platform, ResolutionStatus},
};
use uuid::Uuid;

pub(super) struct ResolvedPerson {
    pub(super) id: Uuid,
    pub(super) created: bool,
}

pub(super) type ResolvedPeople = Vec<Option<ResolvedPerson>>;

pub(super) async fn import_people(
    tx: &mut sqlx::PgConnection,
    export: &OrgExport,
    result: &mut OrgImportResult,
) -> Result<ResolvedPeople, Error> {
    let mut resolved = Vec::with_capacity(export.people.len());

    for person in &export.people {
        let email = person
            .email
            .as_deref()
            .map(str::trim)
            .filter(|email| !email.is_empty());
        let existing = sqlx::query!(
            r#"
            SELECT id, directory_id
            FROM org.people
            WHERE id = $1
               OR ($2::text IS NOT NULL AND directory_id = $2)
               OR ($3::text IS NOT NULL AND lower(btrim(email)) = lower($3))
            FOR UPDATE
            "#,
            person.id,
            person.directory_id,
            email,
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(Error::from)?;

        let mut matches: Vec<Uuid> = existing.iter().map(|row| row.id).collect();
        let conflicting_directory = existing.first().is_some_and(|row| {
            person.directory_id.is_some()
                && row.directory_id.is_some()
                && row.directory_id != person.directory_id
        });
        if matches.is_empty() && person.id.is_none() && person.directory_id.is_none() {
            matches = sqlx::query_scalar!(
                "SELECT id FROM org.people WHERE name = $1 FOR UPDATE",
                person.name
            )
            .fetch_all(&mut *tx)
            .await
            .map_err(Error::from)?;
        }

        let resolved_person = match matches.as_slice() {
            [] => {
                let id = person.id.unwrap_or_else(Uuid::now_v7);
                insert_person(tx, person, id).await?;
                restore_manual_platforms(tx, person, id).await?;

                result.people_created += 1;
                Some(ResolvedPerson { id, created: true })
            }
            [id] if !conflicting_directory => {
                result.people_updated += 1;
                Some(ResolvedPerson {
                    id: *id,
                    created: false,
                })
            }
            _ => {
                result.warnings.push(format!(
                    "Ambiguous or conflicting person match for {} — skipped",
                    person.name
                ));
                None
            }
        };
        resolved.push(resolved_person);
    }

    Ok(resolved)
}

async fn insert_person(
    tx: &mut sqlx::PgConnection,
    person: &ExportPerson,
    id: Uuid,
) -> Result<(), Error> {
    let last_import_at = person
        .last_import_at
        .as_deref()
        .map(|value| {
            time::OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
                .map_err(|_| Error::Validation("invalid exported import timestamp".into()))
        })
        .transpose()?;
    sqlx::query!(
        r#"
        INSERT INTO org.people (id, name, email, level, active, directory_id,
                               last_import_at, membership_management)
        VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
        "#,
        id,
        person.name,
        person.email,
        person.level,
        person.active,
        person.directory_id,
        last_import_at,
        person.membership_management as Management,
    )
    .execute(&mut *tx)
    .await
    .map_err(Error::from)?;

    Ok(())
}

async fn restore_manual_platforms(
    tx: &mut sqlx::PgConnection,
    person: &ExportPerson,
    id: Uuid,
) -> Result<(), Error> {
    let platforms: Vec<String> = person
        .manual_identity_platforms
        .iter()
        .chain(
            person
                .identities
                .iter()
                .filter(|identity| identity.management == Management::Manual)
                .map(|identity| &identity.platform),
        )
        .map(|platform| {
            platform
                .parse::<Platform>()
                .map(|platform| platform.to_string())
                .map_err(|_| Error::Validation("invalid exported platform".into()))
        })
        .collect::<Result<_, _>>()?;
    if platforms.is_empty() {
        return Ok(());
    }
    let status = ResolutionStatus::Manual;
    sqlx::query!(
        r#"
        INSERT INTO org.identity_resolutions (person_id, platform, status)
        SELECT $1, platform, $3
        FROM UNNEST($2::text[]) AS input(platform)
        GROUP BY platform
        ON CONFLICT (person_id, platform) DO NOTHING
        "#,
        id,
        &platforms,
        status as ResolutionStatus,
    )
    .execute(&mut *tx)
    .await
    .map_err(Error::from)?;

    Ok(())
}

pub(super) async fn import_identities(
    tx: &mut sqlx::PgConnection,
    export: &OrgExport,
    result: &mut OrgImportResult,
    resolved: &ResolvedPeople,
) -> Result<(), Error> {
    for (person, resolved) in export.people.iter().zip(resolved) {
        let Some(resolved) = resolved else {
            continue;
        };

        for identity in &person.identities {
            let platform = identity
                .platform
                .parse::<Platform>()
                .map_err(|_| Error::Validation("invalid exported platform".into()))?;
            let username = identity.username.trim().to_lowercase();
            if username.is_empty() {
                return Err(Error::Validation("empty exported username".into()));
            }

            let user_id = identity.platform_user_id.as_deref().map(str::trim);
            if user_id.is_some_and(str::is_empty)
                || (platform == Platform::Jira
                    && identity.management == Management::Manual
                    && user_id.is_none())
            {
                return Err(Error::Validation(
                    "invalid exported platform account ID".into(),
                ));
            }

            let platform = platform.to_string();
            if !resolved.created && platform_is_manual(tx, resolved.id, &platform).await? {
                result.warnings.push(format!(
                    "Manual account choice for {platform} on {} — preserved",
                    person.name
                ));
                continue;
            }

            let id = Uuid::now_v7();
            let rows = sqlx::query!(
                r#"
                INSERT INTO org.platform_identities (id, person_id, platform, platform_username,
                                                    platform_user_id, management)
                VALUES ($1,$2,$3,$4,$5,$6)
                ON CONFLICT DO NOTHING
                "#,
                id,
                resolved.id,
                platform,
                username,
                user_id,
                identity.management as Management,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
            if rows.rows_affected() == 0 {
                result.warnings.push(format!("Account already exists: {platform}/{username} — preserved existing ownership and metadata"));
                continue;
            }

            result.identities_created += 1;
            if identity.management == Management::Manual {
                let status = ResolutionStatus::Manual;
                sqlx::query!(
                    r#"
                    INSERT INTO org.identity_resolutions (person_id,platform,status)
                    VALUES ($1,$2,$3)
                    ON CONFLICT (person_id,platform) DO UPDATE
                    SET status=EXCLUDED.status
                    "#,
                    resolved.id,
                    platform,
                    status as ResolutionStatus,
                )
                .execute(&mut *tx)
                .await
                .map_err(Error::from)?;
            }
        }
    }

    Ok(())
}

async fn platform_is_manual(
    tx: &mut sqlx::PgConnection,
    person_id: Uuid,
    platform: &str,
) -> Result<bool, Error> {
    let manual = Management::Manual;
    let status = ResolutionStatus::Manual;
    sqlx::query_scalar!(
        r#"
        SELECT EXISTS (
            SELECT 1
            FROM org.platform_identities
            WHERE person_id = $1
              AND platform = $2
              AND management = $3
            UNION ALL
            SELECT 1
            FROM org.identity_resolutions
            WHERE person_id = $1
              AND platform = $2
              AND status = $4
        ) AS "protected!"
        "#,
        person_id,
        platform,
        manual as Management,
        status as ResolutionStatus,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(Error::from)
}
