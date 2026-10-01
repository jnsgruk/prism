mod person_activity;
mod person_reviews;
mod snapshots;
mod sources;
mod team_activity;
mod team_reviews;
mod types;

pub use types::*;

use crate::Error;

use sqlx::PgPool;
use time::{Date, OffsetDateTime};
use uuid::Uuid;

/// Repository for read-only enrichment aggregation queries.
///
/// Consumes `reasoning.enrichments` joined with `activity.contributions`,
/// `org.people`, and `org.team_memberships` to produce insight summaries
/// for teams, individuals, and org-wide views.
#[derive(Clone)]
pub struct InsightsRepo {
    pool: PgPool,
    snapshot_period_end: Option<Date>,
}

impl InsightsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self {
            pool,
            snapshot_period_end: None,
        }
    }

    pub fn for_snapshot_period(&self, period_end: Date) -> Self {
        Self {
            pool: self.pool.clone(),
            snapshot_period_end: Some(period_end),
        }
    }
}
