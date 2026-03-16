//! Typed metrics layer over the JSONB `metrics` and `metadata` columns.
//!
//! Each variant of [`ContributionData`] maps to a [`ContributionType`] and
//! provides compile-time field access while serializing to/from
//! `serde_json::Value` for database storage.
//!
//! Phase 1 stored metrics/metadata as raw `serde_json::Value`.  Phase 2
//! introduces these typed structs that round-trip through the same JSONB
//! columns — existing data deserializes into the `PullRequest` and `PrReview`
//! variants transparently.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Top-level tagged enum
// ---------------------------------------------------------------------------

/// Typed metrics layer.  Serializes as `{"type": "pull_request", ...}` via
/// the `serde(tag = "type")` attribute.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContributionData {
    PullRequest(PullRequestData),
    PrReview(PrReviewData),
    JiraTicket(JiraTicketData),
    DiscoursePost(DiscoursePostData),
    DiscourseTopic(DiscourseTopicData),
    DiscourseLike(DiscourseLikeData),
}

// ---------------------------------------------------------------------------
// GitHub — PullRequest
// ---------------------------------------------------------------------------

/// Typed metrics + metadata for a pull request contribution.
///
/// Formalises the JSONB shape already produced by the GitHub source in Phase 1.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PullRequestData {
    // Metrics
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additions: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deletions: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_files: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_count: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub draft: Option<bool>,

    // Metadata
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_number: Option<u32>,
}

// ---------------------------------------------------------------------------
// GitHub — PrReview
// ---------------------------------------------------------------------------

/// Typed metrics + metadata for a PR review contribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PrReviewData {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_hours: Option<f64>,
    /// The `platform_id` of the PR this review belongs to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_platform_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
}

// ---------------------------------------------------------------------------
// Jira — JiraTicket
// ---------------------------------------------------------------------------

/// Typed metrics + metadata for a Jira ticket contribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct JiraTicketData {
    /// Issue type: Bug, Story, Task, Epic, Sub-task, etc.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issue_type: Option<String>,
    /// Story points (field name varies per instance, configured in source settings).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub story_points: Option<f64>,
    /// Computed cycle time: first In Progress → Done, in hours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cycle_time_hours: Option<f64>,
    /// Priority: Highest, High, Medium, Low, Lowest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    /// Issue labels.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<String>,
    /// Parent issue key for sub-tasks (e.g. `"PROJ-100"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_key: Option<String>,
}

// ---------------------------------------------------------------------------
// Discourse — DiscoursePost
// ---------------------------------------------------------------------------

/// Typed metrics + metadata for a Discourse post contribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DiscoursePostData {
    /// Parent topic ID within the Discourse instance.
    #[serde(default)]
    pub topic_id: i64,
    /// Number of direct replies to this post.
    #[serde(default)]
    pub reply_count: i32,
    /// Number of likes on this post.
    #[serde(default)]
    pub likes: i32,
    /// Position in thread (1 = original post).
    #[serde(default)]
    pub post_number: i32,
    /// The post number this post replies to, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply_to_post_number: Option<i32>,
    /// Whether this post is a reply to another post.
    #[serde(default)]
    pub is_reply: bool,
}

// ---------------------------------------------------------------------------
// Discourse — DiscourseTopic
// ---------------------------------------------------------------------------

/// Typed metrics + metadata for a Discourse topic contribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DiscourseTopicData {
    /// Number of posts in the topic.
    #[serde(default)]
    pub post_count: i32,
    /// Number of views.
    #[serde(default)]
    pub views: i32,
    /// Category name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Whether the topic has an accepted answer.
    #[serde(default)]
    pub solved: bool,
}

// ---------------------------------------------------------------------------
// Discourse — DiscourseLike
// ---------------------------------------------------------------------------

/// Typed metrics + metadata for a Discourse like contribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct DiscourseLikeData {
    /// The ID of the liked post.
    pub post_id: i64,
    /// The topic containing the liked post.
    pub topic_id: i64,
    /// Position of the liked post in its topic.
    pub post_number: i32,
    /// Username of the post author (the person who received the like).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_author: Option<String>,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

impl ContributionData {
    /// Serialize to a `serde_json::Value` suitable for the JSONB column.
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }

    /// Attempt to deserialize from a `serde_json::Value`.
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        serde_json::from_value(value.clone()).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_pull_request() {
        let data = ContributionData::PullRequest(PullRequestData {
            additions: Some(42),
            deletions: Some(10),
            changed_files: Some(3),
            review_count: Some(2),
            draft: Some(false),
            repo: Some("canonical/prism".into()),
            head_ref: Some("feature/jira".into()),
            base_ref: Some("main".into()),
            labels: Some(vec!["enhancement".into()]),
            pr_number: Some(123),
        });
        let json = data.to_json();
        assert_eq!(json["type"], "pull_request");
        assert_eq!(json["additions"], 42);
        let back = ContributionData::from_json(&json).unwrap();
        assert_eq!(data, back);
    }

    #[test]
    fn round_trip_pr_review() {
        let data = ContributionData::PrReview(PrReviewData {
            review_state: Some("APPROVED".into()),
            review_hours: Some(2.5),
            pr_platform_id: Some("canonical/prism/pull/123".into()),
            repo: Some("canonical/prism".into()),
        });
        let json = data.to_json();
        assert_eq!(json["type"], "pr_review");
        let back = ContributionData::from_json(&json).unwrap();
        assert_eq!(data, back);
    }

    #[test]
    fn round_trip_jira_ticket() {
        let data = ContributionData::JiraTicket(JiraTicketData {
            issue_type: Some("Story".into()),
            story_points: Some(5.0),
            cycle_time_hours: Some(48.0),
            priority: Some("High".into()),
            labels: vec!["backend".into(), "api".into()],
            parent_key: None,
        });
        let json = data.to_json();
        assert_eq!(json["type"], "jira_ticket");
        assert_eq!(json["story_points"], 5.0);
        let back = ContributionData::from_json(&json).unwrap();
        assert_eq!(data, back);
    }

    #[test]
    fn round_trip_discourse_post() {
        let data = ContributionData::DiscoursePost(DiscoursePostData {
            topic_id: 1234,
            reply_count: 5,
            likes: 12,
            post_number: 3,
            reply_to_post_number: Some(1),
            is_reply: true,
        });
        let json = data.to_json();
        assert_eq!(json["type"], "discourse_post");
        assert_eq!(json["reply_to_post_number"], 1);
        assert!(json["is_reply"].as_bool().unwrap());
        let back = ContributionData::from_json(&json).unwrap();
        assert_eq!(data, back);
    }

    #[test]
    fn round_trip_discourse_post_backward_compat() {
        // Old data without reply fields should deserialise with defaults.
        let json = serde_json::json!({
            "type": "discourse_post",
            "topic_id": 999,
            "reply_count": 2,
            "likes": 5,
            "post_number": 1,
        });
        let data = ContributionData::from_json(&json).unwrap();
        match data {
            ContributionData::DiscoursePost(ref d) => {
                assert_eq!(d.reply_to_post_number, None);
                assert!(!d.is_reply);
            }
            _ => panic!("expected DiscoursePost"),
        }
    }

    #[test]
    fn round_trip_discourse_like() {
        let data = ContributionData::DiscourseLike(DiscourseLikeData {
            post_id: 42,
            topic_id: 10,
            post_number: 3,
            post_author: Some("alice".into()),
        });
        let json = data.to_json();
        assert_eq!(json["type"], "discourse_like");
        assert_eq!(json["post_id"], 42);
        assert_eq!(json["post_author"], "alice");
        let back = ContributionData::from_json(&json).unwrap();
        assert_eq!(data, back);
    }

    #[test]
    fn round_trip_discourse_topic() {
        let data = ContributionData::DiscourseTopic(DiscourseTopicData {
            post_count: 15,
            views: 342,
            category: Some("Development".into()),
            solved: true,
        });
        let json = data.to_json();
        assert_eq!(json["type"], "discourse_topic");
        assert!(json["solved"].as_bool().unwrap());
        let back = ContributionData::from_json(&json).unwrap();
        assert_eq!(data, back);
    }

    #[test]
    fn unknown_type_returns_none() {
        let json = serde_json::json!({"type": "unknown", "foo": 42});
        assert!(ContributionData::from_json(&json).is_none());
    }
}
