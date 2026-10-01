use ps_core::ingestion::ContributionInput;
use ps_core::models::Platform;

use super::super::client::{
    DiscourseClient, Post, PostActionUser, TopicDetailResponse, TopicSummary, UserAction,
    UserActionType,
};
use super::Cursor;
use super::inputs::{
    build_like_input, build_post_input, build_topic_input, parse_discourse_datetime,
};
use super::person::PersonCursor;
use crate::infra::retry::retry_transient;

pub(super) async fn fetch_item(
    client: &DiscourseClient,
    cursor: &PersonCursor,
    action: &UserAction,
) -> Result<Option<ContributionInput>, ps_core::Error> {
    let identity =
        cursor.request.source.identity.as_ref().ok_or_else(|| {
            ps_core::Error::Validation("saved Discourse identity required".into())
        })?;
    if !action
        .acting_username
        .eq_ignore_ascii_case(&identity.username)
        || action.action_type == UserActionType::Other
        || (action.action_type == UserActionType::LikeGiven && !cursor.fetch_likes)
    {
        return Ok(None);
    }
    if action.deleted || action.hidden == Some(true) {
        return Err(ps_core::Error::Validation(
            "activity references a hidden/deleted post; coverage limited".into(),
        ));
    }
    if action.topic_id <= 0 {
        return Err(ps_core::Error::Validation(
            "activity has an invalid topic ID".into(),
        ));
    }
    let (post, detail) = fetch_detail(client, action).await?;
    let post_id = action.post_id.unwrap_or(post.id);
    if post.id != post_id || post.topic_id != action.topic_id || detail.id != action.topic_id {
        return Err(ps_core::Error::Validation(
            "activity detail IDs do not match".into(),
        ));
    }
    if post.hidden
        || post.deleted_at.is_some()
        || detail.visible == Some(false)
        || detail.archetype.as_deref() == Some("private_message")
    {
        return Err(ps_core::Error::Validation(
            "activity detail is hidden/deleted/private; coverage limited".into(),
        ));
    }
    if detail.posts_count < cursor.min_posts
        || (!cursor.category_ids.is_empty()
            && detail
                .category_id
                .is_none_or(|id| !cursor.category_ids.contains(&id)))
    {
        return Ok(None);
    }
    let topic = TopicSummary {
        id: detail.id,
        title: detail.title,
        slug: detail.slug,
        posts_count: detail.posts_count,
        views: detail.views,
        category_id: detail.category_id,
        created_at: detail.created_at,
        bumped_at: detail.bumped_at,
        last_posted_at: None,
        pinned: false,
        has_accepted_answer: detail.has_accepted_answer,
        tags: detail.tags,
    };
    let context = contribution_context(cursor)?;
    let created_at = parse_discourse_datetime(&action.created_at)?;
    let mut item = match action.action_type {
        UserActionType::LikeGiven => {
            let liker = PostActionUser {
                id: 0,
                username: identity.username.to_string(),
                name: action.acting_name.clone(),
            };
            let mut item = build_like_input(&liker, &post, &topic, &context);
            extend_metadata(
                &mut item,
                serde_json::json!({
                    "event_time_source": "discourse_user_action", "user_action_type": 1,
                    "post_id": post.id, "event_created_at": action.created_at,
                }),
            );
            item
        }
        UserActionType::TopicCreated | UserActionType::PostAuthored => {
            if !post.username.eq_ignore_ascii_case(&identity.username) {
                return Err(ps_core::Error::Validation(
                    "authored activity's post owner differs from selected identity".into(),
                ));
            }
            // Type 5 can refer to the initial post on some instances. Its stable
            // contribution key remains the topic ID, never a duplicate post.
            if post.post_number == 1 {
                let category = topic
                    .category_id
                    .and_then(|id| cursor.category_map.get(&id));
                let mut item = build_topic_input(&topic, &context, category.map(String::as_str));
                item.platform_username = identity.username.to_lowercase().into();
                item.content.clone_from(&post.raw);
                if let Some(metrics) = item.metrics.as_object_mut() {
                    metrics.insert("likes".into(), post.likes().into());
                }
                extend_metadata(
                    &mut item,
                    serde_json::json!({ "tags": topic.tags,
                    "username": identity.username.to_lowercase(), "post_id": post.id }),
                );
                if let Some(raw) = &post.raw {
                    item.enrichment_content = Some(serde_json::json!({ "title": topic.title,
                        "category": category, "tags": topic.tags, "body": raw }));
                }
                item
            } else {
                if action.action_type == UserActionType::TopicCreated {
                    return Err(ps_core::Error::Validation(
                        "topic activity references a noninitial post".into(),
                    ));
                }
                let mut item = build_post_input(&post, &topic, &context);
                extend_metadata(
                    &mut item,
                    serde_json::json!({ "category_id": topic.category_id,
                    "tags": topic.tags }),
                );
                item
            }
        }
        UserActionType::Other => return Ok(None),
    };
    item.created_at = created_at;
    item.platform_username = identity.username.to_lowercase().into();
    extend_metadata(
        &mut item,
        serde_json::json!({ "user_action_id": action.id,
        "user_action_key": action.key(), "coverage": cursor.coverage }),
    );
    Ok(Some(item))
}

async fn fetch_detail(
    client: &DiscourseClient,
    action: &UserAction,
) -> Result<(Post, TopicDetailResponse), ps_core::Error> {
    if let Some(post_id) = action.post_id.filter(|id| *id > 0) {
        tokio::try_join!(
            retry_transient(
                "discourse activity post",
                ps_core::Error::is_transient,
                || client.post(post_id)
            ),
            retry_transient(
                "discourse activity topic",
                ps_core::Error::is_transient,
                || client.topic(action.topic_id)
            ),
        )
    } else if action.action_type == UserActionType::TopicCreated {
        // Official topic-creation actions have post_id=null. Only a topic's
        // initial post is safe to recover from its first embedded stream.
        let detail = retry_transient(
            "discourse authored topic",
            ps_core::Error::is_transient,
            || client.topic(action.topic_id),
        )
        .await?;
        let post = detail
            .post_stream
            .as_ref()
            .and_then(|stream| stream.posts.iter().find(|post| post.post_number == 1))
            .cloned()
            .ok_or_else(|| {
                ps_core::Error::Validation(
                    "initial topic post unavailable; coverage limited".into(),
                )
            })?;
        Ok((post, detail))
    } else {
        Err(ps_core::Error::Validation(
            "activity post ID unavailable; coverage limited".into(),
        ))
    }
}

fn extend_metadata(item: &mut ContributionInput, fields: serde_json::Value) {
    if let (serde_json::Value::Object(metadata), serde_json::Value::Object(fields)) =
        (&mut item.metadata, fields)
    {
        metadata.extend(fields);
    }
}

fn contribution_context(cursor: &PersonCursor) -> Result<Cursor, ps_core::Error> {
    let Platform::Discourse(instance) = &cursor.request.source.platform else {
        return Err(ps_core::Error::Validation(
            "person target is not Discourse".into(),
        ));
    };
    Ok(Cursor {
        watermark: None,
        page: 0,
        category_ids: cursor.category_ids.clone(),
        category_index: 0,
        min_posts: cursor.min_posts,
        base_url: cursor.base_url.clone(),
        instance: instance.clone(),
        max_bumped_at: None,
        has_more: true,
        category_map: cursor
            .category_map
            .iter()
            .map(|(id, name)| (*id, name.clone()))
            .collect(),
        failed_items: vec![],
    })
}
