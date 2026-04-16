mod contributions;
mod invocations;
mod pipelines;
mod runs;
mod status;
mod watermarks;

use crate::models;
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

/// Repository for the `activity` schema: contributions, ingestion watermarks,
/// `ETag` cache, and ingestion runs.
#[derive(Clone)]
pub struct ActivityRepo {
    pool: PgPool,
}

/// A row from `activity.ingestion_runs`.
pub struct IngestionRunRow {
    pub id: Uuid,
    pub source_name: String,
    pub started_at: OffsetDateTime,
    pub completed_at: Option<OffsetDateTime>,
    pub status: models::IngestionStatus,
    pub items_collected: Option<i32>,
    pub error_message: Option<String>,
    pub handler_name: String,
    pub handler_method: String,
    pub pipeline_id: Option<Uuid>,
}

/// A joined row from `config.source_configs` + `activity.ingestion_watermarks`.
pub struct SourceStatusRow {
    pub name: String,
    pub source_type: models::Platform,
    pub watermark_value: Option<String>,
    pub last_successful_run: Option<OffsetDateTime>,
    pub last_attempt: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    pub items_collected_last_run: Option<i32>,
    /// Whether this source has a currently running ingestion (no `completed_at`).
    pub has_active_run: bool,
    /// Items collected so far in the active run (from `ingestion_runs`).
    pub active_run_items: Option<i32>,
    /// When the active run started.
    pub active_run_started_at: Option<OffsetDateTime>,
    /// Current Restate invocation ID (for reconciliation).
    pub current_invocation_id: Option<String>,
    /// Structured progress JSON from the active run.
    pub active_run_progress: Option<serde_json::Value>,
}

impl ActivityRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
