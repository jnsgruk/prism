mod contributions;
mod discourse;
mod person;
mod snapshots;
mod sources;

pub use contributions::{
    ContributionDetailRow, ListContributionsParams, ListPersonContributionsParams,
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
}
