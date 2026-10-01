use time::Date;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Review depth distribution and sentiment counts for a scope.
pub struct ReviewQualityRow {
    pub avg_depth: f64,
    pub total_reviews: i32,
    pub depth_1: i32,
    pub depth_2: i32,
    pub depth_3: i32,
    pub depth_4: i32,
    pub depth_5: i32,
    pub constructive: i32,
    pub neutral: i32,
    pub critical: i32,
    pub hostile: i32,
}

/// A top reviewer by average depth.
pub struct ReviewerDepthRow {
    pub person_id: Uuid,
    pub person_name: String,
    pub review_count: i32,
    pub avg_depth: f64,
}

/// PR significance counts.
pub struct SignificanceRow {
    pub significant: i32,
    pub notable: i32,
    pub routine: i32,
    pub avg_confidence: f64,
}

/// Discourse topic category with count.
pub struct TopicCategoryRow {
    pub category: String,
    pub count: i32,
}

/// A notable/exemplary contribution surfaced by enrichments.
pub struct NotableContributionRow {
    pub contribution_id: Uuid,
    pub title: String,
    pub url: String,
    pub person_name: String,
    pub platform: String,
    pub contribution_type: String,
    pub enrichment_type: String,
    pub value_summary: String,
    pub rationale: String,
    pub confidence: f64,
}

/// Coverage stats for a single enrichment type.
pub struct TypeCoverageRow {
    pub enrichment_type: String,
    pub eligible: i32,
    pub enriched: i32,
}

/// Depth × significance cross-reference.
pub struct DepthBySignificanceRow {
    pub avg_depth_significant: f64,
    pub avg_depth_notable: f64,
    pub avg_depth_routine: f64,
    pub significant_review_count: i32,
    pub notable_review_count: i32,
    pub routine_review_count: i32,
}

/// Reviews received summary for an individual.
pub struct ReviewsReceivedRow {
    pub avg_depth_received: f64,
    pub total_reviews_received: i32,
    pub deep_review_pct: f64,
}

/// Parameters for upserting an insight snapshot.
pub struct UpsertSnapshotParams {
    pub team_id: Uuid,
    pub period_start: Date,
    pub period_end: Date,
    pub period_type: String,
    pub avg_review_depth: Option<f32>,
    pub review_count: i32,
    pub rubber_stamp_pct: Option<f32>,
    pub deep_review_pct: Option<f32>,
    pub depth_distribution: Vec<i32>,
    pub constructive_count: i32,
    pub neutral_count: i32,
    pub critical_count: i32,
    pub hostile_count: i32,
    pub significant_count: i32,
    pub notable_count: i32,
    pub routine_count: i32,
    pub avg_depth_on_significant: Option<f32>,
    pub avg_depth_on_notable: Option<f32>,
    pub avg_depth_on_routine: Option<f32>,
    pub enrichment_coverage: serde_json::Value,
    pub raw_insights: serde_json::Value,
}

/// A stored insight snapshot for a team and period.
pub struct SnapshotRow {
    pub avg_review_depth: Option<f32>,
    pub review_count: i32,
    pub rubber_stamp_pct: Option<f32>,
    pub deep_review_pct: Option<f32>,
    pub significant_count: i32,
    pub notable_count: i32,
    pub routine_count: i32,
}

/// Enrichment-based peer percentile for a single metric.
pub struct EnrichmentPeerPercentile {
    pub metric_name: String,
    pub value: f64,
    pub percentile: f64,
    pub peer_count: i32,
}
