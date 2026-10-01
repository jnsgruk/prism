use uuid::Uuid;

use crate::{
    Error,
    models::{HandlerMethod, HandlerName, SourceName},
};

use super::ActivityRepo;

pub struct PipelineRunParams<'a> {
    pub run_id: Uuid,
    pub source_name: &'a SourceName,
    pub handler_name: &'a HandlerName,
    pub method: &'a HandlerMethod,
    pub pipeline_id: Uuid,
    pub invocation_id: &'a str,
}

pub struct PipelineInvocationParams<'a> {
    pub pipeline_id: Uuid,
    pub invocation_id: &'a str,
    pub parent_invocation_id: Option<&'a str>,
    pub kind: &'a str,
    pub run_id: Option<Uuid>,
}

impl ActivityRepo {
    /// Lock the owner so admission and cancellation cannot race registration.
    pub async fn register_pipeline_invocation(
        &self,
        params: PipelineInvocationParams<'_>,
    ) -> Result<bool, Error> {
        let mut transaction = self.pool.begin().await?;
        let active = sqlx::query_scalar!(
            r#"
            SELECT (status IN ('pending', 'running') AND NOT cancellation_requested) AS "active!"
            FROM activity.pipelines WHERE id = $1 FOR UPDATE
            "#,
            params.pipeline_id,
        )
        .fetch_optional(&mut *transaction)
        .await?
        .unwrap_or(false);

        if active {
            sqlx::query!(
                r#"
                INSERT INTO activity.pipeline_invocations
                    (pipeline_id, invocation_id, parent_invocation_id, kind, run_id)
                VALUES ($1, $2, $3, $4, $5)
                ON CONFLICT (pipeline_id, invocation_id) DO UPDATE SET
                    parent_invocation_id = COALESCE(activity.pipeline_invocations.parent_invocation_id, EXCLUDED.parent_invocation_id),
                    run_id = COALESCE(activity.pipeline_invocations.run_id, EXCLUDED.run_id),
                    kind = EXCLUDED.kind
                "#,
                params.pipeline_id,
                params.invocation_id,
                params.parent_invocation_id,
                params.kind,
                params.run_id,
            )
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(active)
    }

    pub async fn create_pipeline_run(&self, params: PipelineRunParams<'_>) -> Result<bool, Error> {
        let mut transaction = self.pool.begin().await?;
        let active = sqlx::query_scalar!(
            "SELECT (status IN ('pending', 'running') AND NOT cancellation_requested) AS \"active!\" FROM activity.pipelines WHERE id = $1 FOR UPDATE",
            params.pipeline_id,
        ).fetch_optional(&mut *transaction).await?.unwrap_or(false);
        if !active {
            return Ok(false);
        }
        sqlx::query!(
            r#"
            INSERT INTO activity.ingestion_runs
                (id, source_name, started_at, status, handler_name, handler_method, pipeline_id)
            SELECT $1, $2, now(), 'running', $3, $4, $5
            FROM activity.pipelines
            WHERE id = $5 AND status IN ('pending', 'running') AND NOT cancellation_requested
            ON CONFLICT (id) DO NOTHING
            "#,
            params.run_id,
            params.source_name.as_str(),
            params.handler_name.as_str(),
            params.method.as_str(),
            params.pipeline_id,
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query!(
            r#"
            INSERT INTO activity.pipeline_invocations (pipeline_id, invocation_id, kind, run_id)
            VALUES ($1, $2, 'ingestion', $3)
            ON CONFLICT (pipeline_id, invocation_id) DO UPDATE SET run_id = EXCLUDED.run_id
            "#,
            params.pipeline_id,
            params.invocation_id,
            params.run_id,
        )
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    pub async fn list_pipeline_invocation_ids(&self, id: Uuid) -> Result<Vec<String>, Error> {
        sqlx::query_scalar!(
            "SELECT invocation_id FROM activity.pipeline_invocations WHERE pipeline_id = $1 ORDER BY registered_at",
            id,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(Error::from)
    }
}
