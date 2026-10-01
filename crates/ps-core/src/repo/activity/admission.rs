use uuid::Uuid;

use crate::{Error, models::Pipeline};

use super::ActivityRepo;

impl ActivityRepo {
    /// The partial unique index serializes every All/Person admission.
    pub async fn reserve_pipeline(
        &self,
        id: Uuid,
        request_snapshot: &serde_json::Value,
        requested_by: Uuid,
        username: &str,
    ) -> Result<Pipeline, Error> {
        sqlx::query_as!(
            Pipeline,
            r#"
            INSERT INTO activity.pipelines
                (id, status, request_snapshot, requested_by, requested_by_username)
            VALUES ($1, 'pending', $2, $3, $4)
            RETURNING *
            "#,
            id,
            request_snapshot,
            requested_by,
            username,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|error| error.constraint() == Some("pipelines_one_active"))
            {
                Error::Conflict("a pipeline is already active".into())
            } else {
                Error::from(error)
            }
        })
    }

    pub async fn get_pipeline(&self, id: Uuid) -> Result<Option<Pipeline>, Error> {
        sqlx::query_as!(
            Pipeline,
            "SELECT * FROM activity.pipelines WHERE id = $1",
            id
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)
    }

    /// A short lease prevents concurrent server instances from submitting the
    /// same intent simultaneously. A crash expires the lease automatically.
    pub async fn claim_pipeline_dispatch(&self) -> Result<Option<Pipeline>, Error> {
        sqlx::query_as!(
            Pipeline,
            r#"
            UPDATE activity.pipelines
            SET dispatch_after = now() + interval '30 seconds',
                dispatch_attempts = dispatch_attempts + 1
            WHERE id = (
                SELECT id FROM activity.pipelines
                WHERE status IN ('pending', 'running', 'cancelling')
                  AND dispatch_after <= now()
                ORDER BY started_at
                LIMIT 1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING *
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(Error::from)
    }

    pub async fn acknowledge_pipeline_dispatch(
        &self,
        id: Uuid,
        invocation_id: &str,
    ) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.pipelines
            SET dispatch_acknowledged = true,
                current_invocation_id = COALESCE(current_invocation_id, $2)
            WHERE id = $1 AND status IN ('pending', 'running', 'cancelling')
            "#,
            id,
            invocation_id,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn request_pipeline_cancel(&self, id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.pipelines
            SET cancellation_requested = true, status = 'cancelling', dispatch_after = now()
            WHERE id = $1 AND status IN ('pending', 'running', 'cancelling')
            "#,
            id,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    pub async fn pipeline_cancel_requested(&self, id: Uuid) -> Result<bool, Error> {
        let result = sqlx::query_scalar!(
            "SELECT (cancellation_requested OR status NOT IN ('pending', 'running')) AS \"cancelled!\" FROM activity.pipelines WHERE id = $1",
            id,
        )
        .fetch_optional(&self.pool)
        .await?;

        // An absent reservation cannot authorize work.
        Ok(result.unwrap_or(true))
    }

    pub async fn list_person_pipelines(
        &self,
        person_id: Uuid,
        limit: i64,
    ) -> Result<Vec<Pipeline>, Error> {
        sqlx::query_as!(
            Pipeline,
            r#"
            SELECT * FROM activity.pipelines
            WHERE request_snapshot->'scope'->>'person_id' = $1
            ORDER BY started_at DESC LIMIT $2
            "#,
            person_id.to_string(),
            limit,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)
    }

    /// Close registration after a definitively terminal root without recording
    /// a user cancellation. This survives a crash while descendants drain.
    pub async fn request_pipeline_stop(&self, id: Uuid) -> Result<(), Error> {
        sqlx::query!(
            r#"
            UPDATE activity.pipelines
            SET status = 'cancelling', error = 'Workflow stopped before finalizing'
            WHERE id = $1 AND status IN ('pending', 'running', 'cancelling')
            "#,
            id,
        )
        .execute(&self.pool)
        .await?;

        Ok(())
    }
}
