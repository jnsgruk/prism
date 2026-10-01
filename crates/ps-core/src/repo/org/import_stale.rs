use super::StalePersonRow;
use crate::Error;
use sqlx::PgConnection;
use uuid::Uuid;

/// Find active, import-managed people absent from this import batch (leavers).
///
/// "Import-managed" means `last_import_at IS NOT NULL` — set whenever a person
/// is seen by a directory import. Manually-added people (never imported) have
/// it `NULL` and are therefore never treated as leavers. `now()` is the
/// transaction start time, so people seen in this run (stamped with the same
/// `now()`) are excluded and only strictly-earlier rows are returned.
pub(super) async fn find_stale_people(tx: &mut PgConnection) -> Result<Vec<StalePersonRow>, Error> {
    let rows = sqlx::query!(
        r#"
        SELECT id, name, email
        FROM org.people
        WHERE active = true
          AND last_import_at IS NOT NULL
          AND last_import_at < now()
        ORDER BY name
        "#,
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(Error::from)?;

    Ok(rows
        .into_iter()
        .map(|r| StalePersonRow {
            id: r.id,
            name: r.name,
            email: r.email,
        })
        .collect())
}

/// Count active, import-managed people — the denominator for the partial-file
/// safety guard.
pub(super) async fn count_active_import_managed(tx: &mut PgConnection) -> Result<i32, Error> {
    sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::int AS "count!"
        FROM org.people
        WHERE active = true AND last_import_at IS NOT NULL
        "#,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(Error::from)
}

/// Deactivate a stale person within the import transaction: clear `active` and
/// end any open team memberships. Mirrors `OrgRepo::deactivate_person` but runs
/// on the shared transaction connection.
pub(super) async fn deactivate_person_in_tx(tx: &mut PgConnection, id: Uuid) -> Result<(), Error> {
    sqlx::query!(
        "UPDATE org.people SET active = false, updated_at = now() WHERE id = $1",
        id,
    )
    .execute(&mut *tx)
    .await
    .map_err(Error::from)?;

    sqlx::query!(
        r#"
        UPDATE org.team_memberships
        SET end_date = CURRENT_DATE
        WHERE person_id = $1 AND (end_date IS NULL OR end_date > CURRENT_DATE)
        "#,
        id,
    )
    .execute(&mut *tx)
    .await
    .map_err(Error::from)?;

    Ok(())
}

/// Count active people with no active team membership.
pub(super) async fn count_unassigned_people(tx: &mut PgConnection) -> Result<i32, Error> {
    sqlx::query_scalar!(
        r#"
        SELECT COUNT(*)::int AS "count!"
        FROM org.people p
        WHERE p.active = true
          AND NOT EXISTS (
              SELECT 1 FROM org.team_memberships tm
              WHERE tm.person_id = p.id
                AND (tm.end_date IS NULL OR tm.end_date > CURRENT_DATE)
          )
        "#,
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(Error::from)
}
