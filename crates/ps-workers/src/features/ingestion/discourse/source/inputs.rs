use super::super::client::{Post, PostActionUser, TopicSummary};
use super::Cursor;
use ps_core::ingestion::ContributionInput;
use ps_core::models::{
    ContributionType, DiscourseLikeData, DiscoursePostData, DiscourseTopicData, Platform,
};

/// Build a `ContributionInput` for a Discourse topic.
pub(super) fn build_topic_input(
    topic: &TopicSummary,
    cur: &Cursor,
    category_name: Option<&str>,
) -> ContributionInput {
    let platform = Platform::Discourse(cur.instance.clone());
    let url = format!("{}/t/{}/{}", cur.base_url, topic.slug, topic.id);

    let metrics_data = DiscourseTopicData {
        post_count: topic.posts_count,
        views: topic.views,
        category: category_name.map(String::from),
        solved: topic.has_accepted_answer,
    };

    let created_at = parse_discourse_datetime(&topic.created_at)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let updated_at = topic
        .bumped_at
        .as_deref()
        .and_then(|s| parse_discourse_datetime(s).ok());

    ContributionInput {
        platform,
        contribution_type: ContributionType::DiscourseTopic,
        platform_id: topic.id.to_string().into(),
        // Topic creator is not in the summary; will be resolved from the first post
        platform_username: String::new().into(),
        title: Some(topic.title.clone()),
        url: Some(url),
        state: None,
        created_at,
        updated_at,
        closed_at: None,
        metrics: serde_json::to_value(&metrics_data).unwrap_or_default(),
        metadata: serde_json::json!({
            "category_id": topic.category_id,
        }),
        content: None,
        state_history: None,
        enrichment_content: None,
    }
}

/// Build a `ContributionInput` for a Discourse post.
pub(super) fn build_post_input(
    post: &Post,
    topic: &TopicSummary,
    cur: &Cursor,
) -> ContributionInput {
    let platform = Platform::Discourse(cur.instance.clone());
    let url = format!(
        "{}/t/{}/{}/{}",
        cur.base_url, topic.slug, topic.id, post.post_number
    );

    let is_reply = post.reply_to_post_number.is_some();
    let metrics_data = DiscoursePostData {
        topic_id: post.topic_id,
        reply_count: post.reply_count,
        likes: post.likes(),
        post_number: post.post_number,
        reply_to_post_number: post.reply_to_post_number,
        is_reply,
    };

    let created_at = parse_discourse_datetime(&post.created_at)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let updated_at = post
        .updated_at
        .as_deref()
        .and_then(|s| parse_discourse_datetime(s).ok());

    ContributionInput {
        platform,
        contribution_type: ContributionType::DiscoursePost,
        platform_id: post.id.to_string().into(),
        platform_username: post.username.to_lowercase().into(),
        title: Some(topic.title.clone()),
        url: Some(url),
        state: None,
        created_at,
        updated_at,
        closed_at: None,
        metrics: serde_json::to_value(&metrics_data).unwrap_or_default(),
        metadata: serde_json::json!({
            "topic_id": post.topic_id,
            "topic_title": topic.title,
            "post_number": post.post_number,
            "username": post.username.to_lowercase(),
            "display_name": post.name,
        }),
        content: post.raw.clone(),
        state_history: None,
        enrichment_content: None,
    }
}

/// Build a `ContributionInput` for a Discourse like.
pub(super) fn build_like_input(
    liker: &PostActionUser,
    post: &Post,
    topic: &TopicSummary,
    cur: &Cursor,
) -> ContributionInput {
    let platform = Platform::Discourse(cur.instance.clone());
    let url = format!(
        "{}/t/{}/{}/{}",
        cur.base_url, topic.slug, topic.id, post.post_number
    );

    let metrics_data = DiscourseLikeData {
        post_id: post.id,
        topic_id: post.topic_id,
        post_number: post.post_number,
        post_author: Some(post.username.clone()),
    };

    let created_at = parse_discourse_datetime(&post.created_at)
        .unwrap_or_else(|_| time::OffsetDateTime::now_utc());

    ContributionInput {
        platform,
        contribution_type: ContributionType::DiscourseLike,
        platform_id: format!("like-{}-{}", post.id, liker.username.to_lowercase()).into(),
        platform_username: liker.username.to_lowercase().into(),
        title: Some(topic.title.clone()),
        url: Some(url),
        state: None,
        created_at,
        updated_at: None,
        closed_at: None,
        metrics: serde_json::to_value(&metrics_data).unwrap_or_default(),
        metadata: serde_json::json!({
            "post_author": post.username.to_lowercase(),
            "topic_id": post.topic_id,
            "topic_title": topic.title,
            "post_number": post.post_number,
            "username": liker.username.to_lowercase(),
            "display_name": liker.name,
        }),
        content: None,
        state_history: None,
        enrichment_content: None,
    }
}

/// Parse a Discourse ISO 8601 datetime string.
pub(super) fn parse_discourse_datetime(s: &str) -> Result<time::OffsetDateTime, ps_core::Error> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).map_err(
        |error| {
            tracing::warn!(error = %error, "invalid Discourse event timestamp");
            ps_core::Error::Validation("invalid Discourse event timestamp".into())
        },
    )
}
