use super::OrgRepo;
use crate::{
    Error,
    models::{Management, Platform},
};
use sqlx::PgConnection;
use std::collections::HashMap;
use uuid::Uuid;

impl OrgRepo {
    /// Import Jira users by matching account IDs first, then unique emails, and
    /// creating platform identities with `platform_user_id` set to the Jira
    /// `accountId`.
    ///
    /// Returns `(mapped_count, unmatched_count, warnings)`.
    pub async fn import_jira_users(
        &self,
        records: &[crate::directory::JiraUserRecord],
    ) -> Result<(i32, i32, Vec<String>), Error> {
        if records.is_empty() {
            return Ok((0, 0, vec![]));
        }

        let mut tx = self.pool.begin().await.map_err(Error::from)?;

        let people = matching_people(&mut tx, records).await?;

        let mut mapped_count = 0i32;
        let mut unmatched_count = 0i32;
        let mut warnings = Vec::new();

        // Collect matched records for batch upsert
        let mut ids = Vec::new();
        let mut person_ids = Vec::new();
        let mut platforms = Vec::new();
        let mut usernames = Vec::new();
        let mut user_ids = Vec::new();

        let candidates = unique_records(records);
        unmatched_count += candidates.skipped;
        warnings.extend(candidates.warnings);
        for record in candidates.records {
            let email_lower = record.email.trim().to_lowercase();
            let email_matches = people
                .email_candidates
                .get(&email_lower)
                .map(Vec::as_slice)
                .unwrap_or_default();
            let person_id = match matching_person(
                people.account_owners.get(&record.account_id).copied(),
                email_matches,
                &people.active_people,
            ) {
                Ok(id) => id,
                Err(reason) => {
                    unmatched_count += 1;
                    warnings.push(format!(
                        "Jira user {} <{}>: {reason} — skipped",
                        record.display_name, record.email
                    ));
                    continue;
                }
            };

            ids.push(Uuid::now_v7());
            person_ids.push(person_id);
            platforms.push(Platform::Jira.to_string());
            usernames.push(email_lower);
            user_ids.push(record.account_id.clone());
        }

        // Batch upsert platform identities
        if !person_ids.is_empty() {
            let imported = Management::Imported;
            let saved = sqlx::query_scalar!(
                r#"
                WITH incoming AS (
                    SELECT i.*
                    FROM UNNEST($1::uuid[], $2::uuid[], $3::text[], $4::text[], $5::text[])
                        AS i(id, person_id, platform, username, user_id)
                    WHERE NOT EXISTS (
                        SELECT 1 FROM org.identity_resolutions ir
                        WHERE ir.person_id = i.person_id AND ir.platform = i.platform
                          AND ir.status = 'manual'
                    )
                      AND NOT EXISTS (
                        SELECT 1 FROM org.platform_identities pi
                        WHERE pi.platform = i.platform AND pi.person_id <> i.person_id
                          AND (pi.platform_user_id = i.user_id
                            OR (pi.platform_user_id IS NULL AND pi.platform_username = i.username))
                    )
                ), promoted AS (
                    UPDATE org.platform_identities pi
                    SET platform_user_id = i.user_id
                    FROM incoming i
                    WHERE pi.person_id = i.person_id AND pi.platform = i.platform
                      AND pi.platform_username = i.username
                      AND pi.platform_user_id IS NULL AND pi.management = $6
                      AND NOT EXISTS (
                          SELECT 1 FROM org.platform_identities owner
                          WHERE owner.platform = i.platform AND owner.platform_user_id = i.user_id
                      )
                    RETURNING pi.id, pi.platform_user_id
                ), saved AS (
                    INSERT INTO org.platform_identities (id, person_id, platform,
                                                         platform_username, platform_user_id)
                    SELECT i.id, i.person_id, i.platform, i.username, i.user_id
                    FROM incoming i
                    WHERE NOT EXISTS (
                        SELECT 1 FROM promoted WHERE platform_user_id = i.user_id
                    )
                    ON CONFLICT (platform_user_id)
                        WHERE platform = 'jira' AND platform_user_id IS NOT NULL
                    DO UPDATE SET platform_username = EXCLUDED.platform_username
                    WHERE org.platform_identities.management = $6
                      AND org.platform_identities.person_id = EXCLUDED.person_id
                    RETURNING id
                )
                SELECT id AS "id!" FROM promoted
                UNION ALL
                SELECT id AS "id!" FROM saved
                "#,
                &ids,
                &person_ids,
                &platforms,
                &usernames,
                &user_ids,
                imported as Management,
            )
            .fetch_all(&mut *tx)
            .await
            .map_err(import_error)?;

            mapped_count = i32::try_from(saved.len()).unwrap_or(i32::MAX);
            let skipped = ids.len() - saved.len();
            if skipped > 0 {
                warnings.push(format!(
                    "{skipped} Jira account ownership conflicts — protected accounts skipped"
                ));
                unmatched_count += i32::try_from(skipped).unwrap_or(i32::MAX);
            }

            // Backfill person_id on existing Jira contributions whose assignee
            // now has a known identity mapping.
            sqlx::query!(
                r#"
                UPDATE activity.contributions c
                SET person_id = pi.person_id
                FROM org.platform_identities pi
                WHERE c.platform = 'jira'
                  AND c.person_id IS NULL
                  AND pi.platform = 'jira'
                  AND pi.platform_user_id = c.metadata->>'assignee_account_id'
                "#,
            )
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        }

        tx.commit().await.map_err(Error::from)?;

        Ok((mapped_count, unmatched_count, warnings))
    }
}

struct MatchingPeople {
    email_candidates: HashMap<String, Vec<Uuid>>,
    active_people: HashMap<Uuid, bool>,
    account_owners: HashMap<String, Uuid>,
}

async fn matching_people(
    tx: &mut PgConnection,
    records: &[crate::directory::JiraUserRecord],
) -> Result<MatchingPeople, Error> {
    let emails: Vec<String> = records
        .iter()
        .map(|r| r.email.trim().to_lowercase())
        .collect();
    let account_ids: Vec<String> = records.iter().map(|r| r.account_id.clone()).collect();

    // Lock both email candidates and stable account owners, including inactive
    // people. An inactive owner must never be bypassed by email matching.
    let rows = sqlx::query!(
        r#"
        SELECT p.id, LOWER(btrim(p.email)) AS email, p.active
        FROM org.people p
        WHERE LOWER(btrim(p.email)) = ANY($1)
           OR EXISTS (
               SELECT 1 FROM org.platform_identities pi
               WHERE pi.person_id = p.id AND pi.platform = $3
                 AND pi.platform_user_id = ANY($2)
           )
        ORDER BY p.id
        FOR UPDATE OF p
        "#,
        &emails,
        &account_ids,
        Platform::Jira as Platform,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(Error::from)?;

    let mut email_candidates: HashMap<String, Vec<Uuid>> = HashMap::new();
    let mut active_people = HashMap::new();
    for row in rows {
        active_people.insert(row.id, row.active);
        if let Some(email) = row.email {
            email_candidates.entry(email).or_default().push(row.id);
        }
    }

    let accounts = sqlx::query!(
        r#"
        SELECT platform_user_id AS "account_id!", person_id
        FROM org.platform_identities
        WHERE platform = $1 AND platform_user_id = ANY($2)
        ORDER BY id
        FOR UPDATE
        "#,
        Platform::Jira as Platform,
        &account_ids,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(Error::from)?;
    let account_owners: HashMap<String, Uuid> = accounts
        .into_iter()
        .map(|row| (row.account_id, row.person_id))
        .collect();

    Ok(MatchingPeople {
        email_candidates,
        active_people,
        account_owners,
    })
}

/// Stable accounts prove ownership; contradictory email matches require repair.
fn matching_person(
    account_owner: Option<Uuid>,
    email_matches: &[Uuid],
    active_people: &HashMap<Uuid, bool>,
) -> Result<Uuid, &'static str> {
    let id = match account_owner {
        Some(owner) => {
            if email_matches.iter().any(|id| *id != owner) {
                return Err("account ID and email have conflicting ownership");
            }
            owner
        }
        None => match email_matches {
            [id] => *id,
            [] => return Err("no matching person found"),
            _ => return Err("ambiguous email match"),
        },
    };

    match active_people.get(&id) {
        Some(true) => Ok(id),
        _ => Err("matched person is inactive or unavailable"),
    }
}

struct JiraCandidates<'a> {
    records: Vec<&'a crate::directory::JiraUserRecord>,
    skipped: i32,
    warnings: Vec<String>,
}

/// Reject ambiguous ownership inside the CSV before a batch upsert.
fn unique_records(records: &[crate::directory::JiraUserRecord]) -> JiraCandidates<'_> {
    let mut accounts_by_email: HashMap<String, std::collections::HashSet<&str>> = HashMap::new();
    let mut emails_by_account: HashMap<&str, std::collections::HashSet<String>> = HashMap::new();
    for record in records {
        let email = record.email.trim().to_lowercase();
        accounts_by_email
            .entry(email.clone())
            .or_default()
            .insert(&record.account_id);
        emails_by_account
            .entry(&record.account_id)
            .or_default()
            .insert(email);
    }

    let mut result = JiraCandidates {
        records: Vec::new(),
        skipped: 0,
        warnings: Vec::new(),
    };
    let mut seen = std::collections::HashSet::new();

    for record in records {
        let email = record.email.trim().to_lowercase();
        if accounts_by_email
            .get(&email)
            .is_some_and(|ids| ids.len() > 1)
            || emails_by_account
                .get(record.account_id.as_str())
                .is_some_and(|emails| emails.len() > 1)
            || record.account_id.trim().is_empty()
        {
            result.warnings.push(format!(
                "Ambiguous or missing Jira account ID for {} — skipped",
                record.email
            ));
            result.skipped += 1;
        } else if seen.insert((email, record.account_id.as_str())) {
            result.records.push(record);
        }
    }

    result
}

fn import_error(error: sqlx::Error) -> Error {
    if error
        .as_database_error()
        .is_some_and(sqlx::error::DatabaseError::is_unique_violation)
    {
        Error::Conflict("Jira account ownership conflict; no accounts were imported".into())
    } else {
        Error::from(error)
    }
}
