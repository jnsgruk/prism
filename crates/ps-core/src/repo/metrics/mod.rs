mod contributions;
mod discourse;
mod person;
mod snapshots;
mod sources;

pub use contributions::{
    ContributionDetailRow, ContributionFullRow, ListContributionsParams,
    ListPersonContributionsParams,
};
pub use discourse::{DiscourseActivityRow, DiscourseCategoryRow, DiscourseContributorRow};
pub use person::{PeerPercentileRow, PersonActivityRow};
pub use snapshots::{PeriodRow, SnapshotInput, TeamSnapshotRow};
pub use sources::ContributionMetricRow;

use crate::Error;
use crate::models::PeriodType;
use sqlx::PgPool;
use time::Date;
use uuid::Uuid;

/// Repository for the `metrics` schema: pre-computed team snapshots.
#[derive(Clone)]
pub struct MetricsRepo {
    pool: PgPool,
}

impl MetricsRepo {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Delete all metrics data in reverse-FK order.
    ///
    /// Called at the start of a full overwrite restore.
    /// Note: `metrics.team_snapshots` is also deleted by `activity.reset_all()`,
    /// but we delete it here first to ensure ordering is correct.
    pub async fn delete_all_for_restore(&self) -> Result<(), Error> {
        let mut tx: sqlx::Transaction<'_, sqlx::Postgres> =
            self.pool.begin().await.map_err(Error::from)?;

        sqlx::query!("DELETE FROM metrics.snapshot_sources")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM metrics.individual_profiles")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;
        sqlx::query!("DELETE FROM metrics.team_snapshots")
            .execute(&mut *tx)
            .await
            .map_err(Error::from)?;

        tx.commit().await.map_err(Error::from)
    }
}
