use std::collections::HashMap;

use futures::stream::{self, StreamExt};
use ps_core::ingestion::{ContributionInput, FetchResult, IngestionContext};
use ps_core::models::{
    ContributionType, DiscourseLikeData, DiscoursePostData, DiscourseTopicData, Platform,
};
use tracing::{debug, info, warn};

use super::{Cursor, MAX_PAGES_PER_RUN, decrypt_api_key, decrypt_api_username, serialise_cursor};
use crate::discourse::client::{Category, DiscourseClient, Post, PostActionUser, TopicSummary};

pub(super) async fn fetch_batch_impl(
    ctx: &IngestionContext,
    cursor: &str,
) -> Result<FetchResult, ps_core::Error> {
    let mut cur: Cursor = serde_json::from_str(cursor)
        .map_err(|e| ps_core::Error::Internal(format!("invalid cursor: {e}")))?;

    let api_key = decrypt_api_key(ctx).await?;
    let api_username = decrypt_api_username(ctx).await.unwrap_or_default();

    let settings = &ctx.source_config.settings;
    let base_url = settings
        .get("base_url")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("https://discourse.example.com");

    let fetch_likes = settings
        .get("fetch_likes")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let client = DiscourseClient::new(ctx.http_client.clone(), base_url, &api_key, &api_username);

    // Fetch categories once for name resolution (only on first page).
    let category_map = if cur.page == 0 {
        build_category_map(&client).await.unwrap_or_default()
    } else {
        HashMap::new()
    };

    // Fetch the latest topics page
    let response = client.latest(cur.page).await?;

    let topics = &response.topic_list.topics;
    let has_more_pages = response.topic_list.more_topics_url.is_some();

    info!(
        source = ctx.source_config.name,
        page = cur.page,
        topics = topics.len(),
        has_more = has_more_pages,
        "fetched Discourse topics page"
    );

    if topics.is_empty() {
        return Ok(FetchResult {
            items: vec![],
            next_cursor: None,
            rate_limit: None,
            etag: None,
        });
    }

    let mut items = Vec::new();
    let mut reached_watermark = false;

    for topic in topics {
        // Check watermark — stop if we've reached topics older than our watermark.
        // Pinned topics appear at the top of /latest.json regardless of their
        // bumped_at timestamp.  Skip them from the watermark boundary check
        // (they'd otherwise stop ingestion prematurely), but still skip old
        // pinned topics we've already ingested.
        let bumped_at = topic.bumped_at.as_deref().or(Some(&topic.created_at));
        let older_than_watermark = matches!(
            (&cur.watermark, bumped_at),
            (Some(wm), Some(bumped)) if bumped <= wm.as_str()
        );
        if older_than_watermark {
            if topic.pinned {
                // Old pinned topic — skip it but keep scanning.
                continue;
            }
            // Non-pinned topic older than watermark — we've reached the boundary.
            reached_watermark = true;
            break;
        }

        // Category filter
        if !cur.category_ids.is_empty() {
            if let Some(cat_id) = topic.category_id {
                if !cur.category_ids.contains(&cat_id) {
                    continue;
                }
            } else {
                continue;
            }
        }

        // Min posts filter
        if topic.posts_count < cur.min_posts {
            continue;
        }

        // Track max bumped_at for watermark advancement
        if let Some(bumped) = bumped_at {
            match cur.max_bumped_at {
                Some(ref current) if bumped > current.as_str() => {
                    cur.max_bumped_at = Some(bumped.to_string());
                }
                None => {
                    cur.max_bumped_at = Some(bumped.to_string());
                }
                _ => {}
            }
        }

        // Fetch full topic detail to get posts
        let detail = match client.topic(topic.id).await {
            Ok(d) => d,
            Err(e) => {
                warn!(topic_id = topic.id, "failed to fetch topic detail: {e}");
                continue;
            }
        };

        let category_name = topic
            .category_id
            .and_then(|id| category_map.get(&id))
            .cloned();

        // Create topic contribution
        items.push(build_topic_input(topic, &cur, category_name.as_deref()));

        // Create post contributions from the topic detail
        if let Some(ref post_stream) = detail.post_stream {
            for post in &post_stream.posts {
                items.push(build_post_input(post, topic, &cur));
            }

            // Fetch likes for posts that have them (opt-in via source settings)
            if fetch_likes {
                let like_items =
                    fetch_likes_for_posts(&client, &post_stream.posts, topic, &cur).await;
                items.extend(like_items);
            }
        }
    }

    let stop = reached_watermark || !has_more_pages || cur.page >= MAX_PAGES_PER_RUN;

    let next_cursor = if stop {
        None
    } else {
        cur.page += 1;
        cur.has_more = has_more_pages;
        Some(serialise_cursor(&cur)?)
    };

    debug!(
        source = ctx.source_config.name,
        items = items.len(),
        stop,
        "processed Discourse batch"
    );

    Ok(FetchResult {
        items,
        next_cursor,
        rate_limit: None,
        etag: None,
    })
}

/// Build a `ContributionInput` for a Discourse topic.
fn build_topic_input(
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
        platform_id: topic.id.to_string(),
        // Topic creator is not in the summary; will be resolved from the first post
        platform_username: String::new(),
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
    }
}

/// Build a `ContributionInput` for a Discourse post.
fn build_post_input(post: &Post, topic: &TopicSummary, cur: &Cursor) -> ContributionInput {
    let platform = Platform::Discourse(cur.instance.clone());
    let url = format!(
        "{}/t/{}/{}/{}",
        cur.base_url, topic.slug, topic.id, post.post_number
    );

    let is_reply = post.reply_to_post_number.is_some();
    let metrics_data = DiscoursePostData {
        topic_id: post.topic_id,
        reply_count: post.reply_count,
        likes: post.like_count,
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
        platform_id: post.id.to_string(),
        platform_username: post.username.clone(),
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
            "username": post.username,
            "display_name": post.name,
        }),
        content: post.raw.clone(),
        state_history: None,
    }
}

/// Fetch likers for all posts with `like_count > 0` in a topic, with capped
/// concurrency, and return the resulting `ContributionInput` items.
async fn fetch_likes_for_posts(
    client: &DiscourseClient,
    posts: &[Post],
    topic: &TopicSummary,
    cur: &Cursor,
) -> Vec<ContributionInput> {
    let likeable_posts: Vec<Post> = posts.iter().filter(|p| p.like_count > 0).cloned().collect();

    if likeable_posts.is_empty() {
        return vec![];
    }

    let topic = topic.clone();
    let cur = cur.clone();

    stream::iter(likeable_posts)
        .map(|post| {
            let topic = &topic;
            let cur = &cur;
            async move {
                match client.post_likers(post.id).await {
                    Ok(likers) => likers
                        .iter()
                        .map(|liker| build_like_input(liker, &post, topic, cur))
                        .collect::<Vec<_>>(),
                    Err(e) => {
                        warn!(post_id = post.id, "failed to fetch post likers: {e}");
                        vec![]
                    }
                }
            }
        })
        .buffer_unordered(5)
        .flat_map(stream::iter)
        .collect()
        .await
}

/// Build a `ContributionInput` for a Discourse like.
fn build_like_input(
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
        platform_id: format!("like-{}-{}", post.id, liker.username),
        platform_username: liker.username.clone(),
        title: Some(topic.title.clone()),
        url: Some(url),
        state: None,
        created_at,
        updated_at: None,
        closed_at: None,
        metrics: serde_json::to_value(&metrics_data).unwrap_or_default(),
        metadata: serde_json::json!({
            "post_author": post.username,
            "topic_id": post.topic_id,
            "topic_title": topic.title,
            "post_number": post.post_number,
            "username": liker.username,
            "display_name": liker.name,
        }),
        content: None,
        state_history: None,
    }
}

/// Build a category ID → name map.
async fn build_category_map(
    client: &DiscourseClient,
) -> Result<HashMap<i64, String>, ps_core::Error> {
    let categories = client.categories().await?;
    Ok(categories
        .into_iter()
        .map(|c: Category| (c.id, c.name))
        .collect())
}

/// Parse a Discourse ISO 8601 datetime string.
fn parse_discourse_datetime(s: &str) -> Result<time::OffsetDateTime, ps_core::Error> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
        .map_err(|e| ps_core::Error::Internal(format!("invalid datetime '{s}': {e}")))
}
