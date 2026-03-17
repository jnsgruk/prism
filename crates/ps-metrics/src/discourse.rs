//! Discourse activity metrics: topics, posts, replies, likes, solved topics.

use std::collections::{HashMap, HashSet};

use ps_core::models::ContributionType;
use ps_core::repo::metrics::ContributionMetricRow;

/// Aggregated Discourse activity metrics for a team snapshot.
#[derive(Debug, Clone)]
pub struct DiscourseMetrics {
    pub topics_created: i32,
    pub posts: i32,
    pub replies: i32,
    pub likes_given: i32,
    pub likes_received: i32,
    pub solved_topics: i32,
    pub active_participants: i32,
    pub by_instance: HashMap<String, DiscourseInstanceMetrics>,
}

/// Per-instance breakdown of Discourse activity.
#[derive(Debug, Clone, Default)]
pub struct DiscourseInstanceMetrics {
    pub topics_created: i32,
    pub posts: i32,
    pub replies: i32,
    pub likes_given: i32,
    pub solved_topics: i32,
}

/// Compute Discourse metrics from the contribution slice already fetched for
/// snapshot computation. No additional DB queries needed.
#[allow(clippy::cast_possible_wrap)]
pub fn compute_discourse_metrics(
    contributions: &[ContributionMetricRow],
) -> Option<DiscourseMetrics> {
    let mut topics_created: i32 = 0;
    let mut posts: i32 = 0;
    let mut replies: i32 = 0;
    let mut likes_given: i32 = 0;
    let mut likes_received: i32 = 0;
    let mut solved_topics: i32 = 0;
    let mut participants: HashSet<uuid::Uuid> = HashSet::new();
    let mut by_instance: HashMap<String, DiscourseInstanceMetrics> = HashMap::new();

    for c in contributions {
        let ps_core::models::Platform::Discourse(instance) = &c.platform else {
            continue;
        };

        if let Some(pid) = c.person_id {
            participants.insert(pid);
        }

        let inst = by_instance.entry(instance.clone()).or_default();

        match c.contribution_type {
            ContributionType::DiscourseTopic => {
                topics_created += 1;
                inst.topics_created += 1;

                if c.metrics
                    .get("solved")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                {
                    solved_topics += 1;
                    inst.solved_topics += 1;
                }
            }
            ContributionType::DiscoursePost => {
                posts += 1;
                inst.posts += 1;

                if c.metrics
                    .get("is_reply")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(false)
                {
                    replies += 1;
                    inst.replies += 1;
                }

                // Likes received on this post
                let post_likes = c
                    .metrics
                    .get("likes")
                    .and_then(serde_json::Value::as_i64)
                    .and_then(|n| i32::try_from(n).ok())
                    .unwrap_or(0);
                likes_received += post_likes;
            }
            ContributionType::DiscourseLike => {
                likes_given += 1;
                inst.likes_given += 1;
            }
            _ => {}
        }
    }

    // Only return metrics if there's any Discourse activity
    if topics_created == 0 && posts == 0 && likes_given == 0 {
        return None;
    }

    Some(DiscourseMetrics {
        topics_created,
        posts,
        replies,
        likes_given,
        likes_received,
        solved_topics,
        active_participants: i32::try_from(participants.len()).unwrap_or(i32::MAX),
        by_instance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ps_core::models::{ContributionState, Platform};
    use time::macros::datetime;
    use uuid::Uuid;

    fn make_discourse(
        platform: Platform,
        contribution_type: ContributionType,
        metrics: serde_json::Value,
        person_id: Option<Uuid>,
    ) -> ContributionMetricRow {
        ContributionMetricRow {
            id: Uuid::now_v7(),
            person_id,
            platform,
            platform_id: Uuid::now_v7().to_string(),
            contribution_type,
            state: None,
            created_at: datetime!(2026-03-01 0:00 UTC),
            closed_at: None,
            metrics,
            metadata: serde_json::json!({}),
            state_history: None,
        }
    }

    #[test]
    fn computes_basic_discourse_metrics() {
        let person_a = Uuid::now_v7();
        let person_b = Uuid::now_v7();
        let ubuntu = Platform::Discourse("ubuntu".into());

        let contributions = vec![
            make_discourse(
                ubuntu.clone(),
                ContributionType::DiscourseTopic,
                serde_json::json!({"solved": true, "views": 100}),
                Some(person_a),
            ),
            make_discourse(
                ubuntu.clone(),
                ContributionType::DiscourseTopic,
                serde_json::json!({"solved": false}),
                Some(person_b),
            ),
            make_discourse(
                ubuntu.clone(),
                ContributionType::DiscoursePost,
                serde_json::json!({"is_reply": true, "likes": 3}),
                Some(person_a),
            ),
            make_discourse(
                ubuntu.clone(),
                ContributionType::DiscoursePost,
                serde_json::json!({"is_reply": false, "likes": 1}),
                Some(person_b),
            ),
            make_discourse(
                ubuntu.clone(),
                ContributionType::DiscourseLike,
                serde_json::json!({}),
                Some(person_a),
            ),
        ];

        let result = compute_discourse_metrics(&contributions).unwrap();
        assert_eq!(result.topics_created, 2);
        assert_eq!(result.posts, 2);
        assert_eq!(result.replies, 1);
        assert_eq!(result.likes_given, 1);
        assert_eq!(result.likes_received, 4);
        assert_eq!(result.solved_topics, 1);
        assert_eq!(result.active_participants, 2);

        let inst = &result.by_instance["ubuntu"];
        assert_eq!(inst.topics_created, 2);
        assert_eq!(inst.posts, 2);
        assert_eq!(inst.replies, 1);
        assert_eq!(inst.likes_given, 1);
        assert_eq!(inst.solved_topics, 1);
    }

    #[test]
    fn multi_instance_breakdown() {
        let person = Uuid::now_v7();
        let contributions = vec![
            make_discourse(
                Platform::Discourse("ubuntu".into()),
                ContributionType::DiscourseTopic,
                serde_json::json!({}),
                Some(person),
            ),
            make_discourse(
                Platform::Discourse("snapcraft".into()),
                ContributionType::DiscoursePost,
                serde_json::json!({"is_reply": true, "likes": 0}),
                Some(person),
            ),
        ];

        let result = compute_discourse_metrics(&contributions).unwrap();
        assert_eq!(result.topics_created, 1);
        assert_eq!(result.posts, 1);
        assert_eq!(result.by_instance.len(), 2);
        assert_eq!(result.by_instance["ubuntu"].topics_created, 1);
        assert_eq!(result.by_instance["snapcraft"].posts, 1);
        assert_eq!(result.active_participants, 1);
    }

    #[test]
    fn none_without_discourse_data() {
        let contributions = vec![ContributionMetricRow {
            id: Uuid::now_v7(),
            person_id: Some(Uuid::nil()),
            platform: Platform::Github,
            platform_id: "PR_1".into(),
            contribution_type: ContributionType::PullRequest,
            state: Some(ContributionState::Merged),
            created_at: datetime!(2026-03-01 0:00 UTC),
            closed_at: None,
            metrics: serde_json::json!({}),
            metadata: serde_json::json!({}),
            state_history: None,
        }];
        assert!(compute_discourse_metrics(&contributions).is_none());
    }

    #[test]
    fn none_for_empty_contributions() {
        assert!(compute_discourse_metrics(&[]).is_none());
    }
}
