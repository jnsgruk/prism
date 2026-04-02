//! Enrichment output types for Rig structured extraction.
//!
//! Each struct represents the structured output from a specific enrichment
//! type. All derive `JsonSchema` (for Rig extractors), `Serialize` +
//! `Deserialize` (for JSONB storage), and live here rather than in `ps-core`
//! because they are reasoning-specific.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Re-export from the canonical location in ps-core.
pub use ps_core::models::EnrichmentType;

// ---------------------------------------------------------------------------
// Review Depth (PR reviews)
// ---------------------------------------------------------------------------

/// Scores the depth of a code review on a 1–5 scale.
///
/// 1 = trivial rubber-stamp (e.g. "LGTM"), 5 = thorough architectural review
/// with substantive technical feedback.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReviewDepthScore {
    /// Score from 1 (trivial/rubber-stamp) to 5 (thorough architectural review).
    pub score: u8,
    /// Brief rationale for the score (1-2 sentences).
    pub rationale: String,
    /// Confidence in the assessment (0.0 to 1.0).
    pub confidence: f32,
}

// ---------------------------------------------------------------------------
// Sentiment (PR reviews)
// ---------------------------------------------------------------------------

/// Labels the tone/sentiment of a code review.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SentimentLabel {
    /// The overall tone of the review.
    pub sentiment: Sentiment,
    /// Brief rationale for the label (1-2 sentences).
    pub rationale: String,
    /// Confidence in the assessment (0.0 to 1.0).
    pub confidence: f32,
}

/// The possible sentiment categories for a code review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Sentiment {
    /// Helpful, encouraging, focused on improving the code.
    Constructive,
    /// Neither positive nor negative — factual or procedural.
    Neutral,
    /// Points out problems but in a professional way.
    Critical,
    /// Aggressive, dismissive, or personal attacks.
    Hostile,
}

// ---------------------------------------------------------------------------
// Significance (Pull Requests)
// ---------------------------------------------------------------------------

/// Categorises how significant a pull request is.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SignificanceLabel {
    /// The significance level of the PR.
    pub significance: Significance,
    /// Brief rationale for the categorisation (1-2 sentences).
    pub rationale: String,
    /// Confidence in the assessment (0.0 to 1.0).
    pub confidence: f32,
}

/// The possible significance levels for a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Significance {
    /// Minor fix, dependency bump, formatting, trivial change.
    Routine,
    /// Meaningful feature work, non-trivial refactor, important bug fix.
    Notable,
    /// Major architectural change, large feature, critical fix.
    Significant,
}

// ---------------------------------------------------------------------------
// Topic Classification (Discourse topics)
// ---------------------------------------------------------------------------

/// Classifies a Discourse topic into categories.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TopicClassification {
    /// Primary topic category.
    pub primary_category: TopicCategory,
    /// Optional secondary category.
    pub secondary_category: Option<TopicCategory>,
    /// Brief rationale for the classification (1-2 sentences).
    pub rationale: String,
    /// Confidence in the assessment (0.0 to 1.0).
    pub confidence: f32,
}

/// The possible topic categories for a Discourse topic.
///
/// These match the categories defined in the topic classification prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TopicCategory {
    Question,
    Announcement,
    Discussion,
    BugReport,
    FeatureRequest,
    Tutorial,
    Showcase,
    Blog,
    Meta,
    Other,
}
