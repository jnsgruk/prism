use super::OrgRepo;
use crate::{
    Error,
    models::{Management, Platform},
};
use std::collections::HashMap;
use uuid::Uuid;

impl OrgRepo {
    /// Import Jira users by matching email addresses to existing people and
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

        // Collect unique emails for batch lookup
        let emails: Vec<String> = records
            .iter()
            .map(|r| r.email.trim().to_lowercase())
            .collect();

        // Look up people by email (case-insensitive)
        let rows = sqlx::query!(
            r#"
            SELECT id, LOWER(btrim(email)) as "email!"
            FROM org.people
            WHERE LOWER(btrim(email)) = ANY($1)
              AND active = true
            ORDER BY id
            FOR UPDATE
            "#,
            &emails,
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(Error::from)?;

        let mut candidates: HashMap<String, Vec<Uuid>> = HashMap::new();
        for row in rows {
            candidates.entry(row.email).or_default().push(row.id);
        }
        let email_to_person: HashMap<String, Uuid> = candidates
            .into_iter()
            .filter_map(|(email, ids)| match ids.as_slice() {
                [id] => Some((email, *id)),
                _ => None,
            })
            .collect();

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
            if let Some(&person_id) = email_to_person.get(&email_lower) {
                ids.push(Uuid::now_v7());
                person_ids.push(person_id);
                platforms.push(Platform::Jira.to_string());
                usernames.push(record.email.trim().to_lowercase());
                user_ids.push(record.account_id.clone());
            } else {
                unmatched_count += 1;
                warnings.push(format!(
                    "No person found for Jira user {} <{}>",
                    record.display_name, record.email
                ));
            }
        }

        // Batch upsert platform identities
        if !person_ids.is_empty() {
            let imported = Management::Imported;
            let saved = sqlx::query_scalar!(
                r#"
                INSERT INTO org.platform_identities (id, person_id, platform, platform_username, platform_user_id)
                SELECT i.id, i.person_id, i.platform, i.username, i.user_id
                FROM UNNEST($1::uuid[], $2::uuid[], $3::text[], $4::text[], $5::text[])
                    AS i(id, person_id, platform, username, user_id)
                WHERE NOT EXISTS (SELECT 1 FROM org.identity_resolutions ir WHERE ir.person_id = i.person_id AND ir.platform = i.platform AND ir.status = 'manual')
                  AND NOT EXISTS (
                    SELECT 1 FROM org.platform_identities pi
                    WHERE pi.platform = i.platform AND pi.platform_user_id = i.user_id
                      AND pi.platform_username <> i.username
                )
                ON CONFLICT (platform, platform_username)
                DO UPDATE SET platform_user_id = EXCLUDED.platform_user_id
                WHERE org.platform_identities.management = $6
                  AND org.platform_identities.person_id = EXCLUDED.person_id
                RETURNING id
                "#, &ids, &person_ids, &platforms, &usernames, &user_ids, imported as Management
            ).fetch_all(&mut *tx).await.map_err(|error| {
                if error.as_database_error().is_some_and(sqlx::error::DatabaseError::is_unique_violation) {
                    Error::Conflict("Jira account ownership conflict; no accounts were imported".into())
                } else { Error::from(error) }
            })?;
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
