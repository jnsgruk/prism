use std::collections::HashMap;

use sqlx::{Postgres, Transaction};
use time::{Date, Month, OffsetDateTime};
use uuid::Uuid;

use crate::{
    Error,
    ingestion::{ContributionInput, SourceRunContext},
    models::PeriodType,
};

pub(super) struct ContributionBefore {
    pub id: Uuid,
    pub person_id: Option<Uuid>,
    pub created_at: OffsetDateTime,
    pub updated_at: Option<OffsetDateTime>,
    pub closed_at: Option<OffsetDateTime>,
    pub input: serde_json::Value,
}

pub(super) async fn read_contributions(
    tx: &mut Transaction<'_, Postgres>,
    request: &SourceRunContext,
    items: &[&ContributionInput],
) -> Result<HashMap<String, ContributionBefore>, Error> {
    let platform = request.source.platform.to_string();
    let keys: Vec<_> = items.iter().map(|item| item.platform_id.as_str()).collect();
    let rows = sqlx::query!(
        r#"
        SELECT id, platform_id, person_id, created_at, updated_at, closed_at,
            (to_jsonb(c) - ARRAY['id','ingested_at']) AS "input!"
        FROM activity.contributions c
        WHERE platform = $1 AND platform_id = ANY($2)
        FOR UPDATE
        "#,
        platform,
        &keys as &[&str],
    )
    .fetch_all(&mut **tx)
    .await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            (
                row.platform_id,
                ContributionBefore {
                    id: row.id,
                    person_id: row.person_id,
                    created_at: row.created_at,
                    updated_at: row.updated_at,
                    closed_at: row.closed_at,
                    input: row.input,
                },
            )
        })
        .collect())
}

pub(super) async fn record_changes(
    tx: &mut Transaction<'_, Postgres>,
    request: &SourceRunContext,
    run_id: Uuid,
    before: &HashMap<String, ContributionBefore>,
    after: &HashMap<String, ContributionBefore>,
    changed: &[(&ContributionInput, Uuid)],
) -> Result<(), Error> {
    if changed.is_empty() {
        return Ok(());
    }

    let mut ids = Vec::new();
    let mut contribution_ids = Vec::new();
    let mut previous_people = Vec::new();
    let mut previous_dates = Vec::new();
    let mut current_dates = Vec::new();
    let mut previous_inputs = Vec::new();
    let mut current_inputs = Vec::new();
    let mut affected_periods = Vec::new();
    let mut hashes = Vec::new();
    for (item, contribution_id) in changed {
        let previous = before.get(item.platform_id.as_str());
        let current = after
            .get(item.platform_id.as_str())
            .ok_or_else(|| Error::Internal("saved contribution missing".into()))?;
        ids.push(Uuid::now_v7());
        contribution_ids.push(*contribution_id);
        previous_people.push(previous.and_then(|row| row.person_id));
        previous_dates.push(previous.map(|row| row.created_at));
        current_dates.push(current.created_at);
        previous_inputs.push(previous.map(|row| &row.input));
        current_inputs.push(&current.input);
        hashes.push(crate::repo::reasoning::content_hash(&current.input));
        let dates = previous
            .into_iter()
            .chain(std::iter::once(current))
            .flat_map(|row| {
                [Some(row.created_at), row.updated_at, row.closed_at]
                    .into_iter()
                    .flatten()
            });
        affected_periods.push(periods_for_dates(dates)?);
    }
    let person_id = request
        .scope
        .person_id()
        .ok_or_else(|| Error::Validation("person required".into()))?
        .into_inner();
    sqlx::query!(
        r#"
        INSERT INTO activity.contribution_changes (
            id, pipeline_id, run_id, source_id, contribution_id,
            previous_person_id, current_person_id, previous_created_at, current_created_at,
            previous_input, current_input, affected_periods, input_hash
        )
        SELECT id, $2, $3, $4, contribution_id, previous_person_id, $5,
            previous_created_at, current_created_at, previous_input, current_input, periods, hash
        FROM UNNEST($1::uuid[], $6::uuid[], $7::uuid[], $8::timestamptz[], $9::timestamptz[],
            $10::jsonb[], $11::jsonb[], $12::jsonb[], $13::text[])
            AS input(id, contribution_id, previous_person_id, previous_created_at, current_created_at,
                previous_input, current_input, periods, hash)
        ON CONFLICT (pipeline_id, contribution_id, input_hash) DO NOTHING
        "#,
        &ids,
        request.pipeline_id,
        run_id,
        request.source.source_id.into_inner(),
        person_id,
        &contribution_ids,
        &previous_people as &[Option<Uuid>],
        &previous_dates as &[Option<OffsetDateTime>],
        &current_dates,
        &previous_inputs as &[Option<&serde_json::Value>],
        &current_inputs as &[&serde_json::Value],
        &affected_periods,
        &hashes,
    )
    .execute(&mut **tx)
    .await?;
    Ok(())
}

fn periods_for_dates(
    dates: impl Iterator<Item = OffsetDateTime>,
) -> Result<serde_json::Value, Error> {
    let mut periods = Vec::new();
    for date in dates.map(|date| date.to_offset(time::UtcOffset::UTC).date()) {
        let month_start = Date::from_calendar_date(date.year(), date.month(), 1)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let quarter_month = Month::try_from((u8::from(date.month()) - 1) / 3 * 3 + 1)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let quarter_start = Date::from_calendar_date(date.year(), quarter_month, 1)
            .map_err(|error| Error::Internal(error.to_string()))?;
        let week_start =
            date - time::Duration::days(i64::from(date.weekday().number_days_from_monday()));
        for (kind, start) in [
            (PeriodType::Week, week_start),
            (PeriodType::Month, month_start),
            (PeriodType::Quarter, quarter_start),
        ] {
            let period =
                serde_json::json!({"period_type":kind.as_str(), "period_start":start.to_string()});
            if !periods.contains(&period) {
                periods.push(period);
            }
        }
    }
    Ok(serde_json::Value::Array(periods))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correction_retains_old_and_new_month_week_and_quarter() {
        let old = OffsetDateTime::parse(
            "2026-03-31T23:59:00Z",
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap();
        let new = old + time::Duration::days(8);
        let periods = periods_for_dates([old, new].into_iter()).unwrap();
        let periods = periods.as_array().unwrap();
        assert_eq!(periods.len(), 6);
        assert!(
            periods
                .contains(&serde_json::json!({"period_type":"month","period_start":"2026-03-01"}))
        );
        assert!(
            periods.contains(
                &serde_json::json!({"period_type":"quarter","period_start":"2026-04-01"})
            )
        );
    }
}
