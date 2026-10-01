use ps_core::ingestion::{ContributionInput, FailedItem, FetchResult, IngestionContext};
use ps_core::models::RateLimitInfo;
use tracing::warn;

use super::super::client::{DiscourseClient, LatestResponse, TopicSummary};
use super::inputs::{ContributionContext, build_like_input, build_post_input, build_topic_input};
use super::{Cursor, MAX_PAGES_PER_RUN, decrypt_api_key, decrypt_api_username, serialise_cursor};
use crate::infra::retry::retry_transient;

pub(super) async fn fetch_batch_impl(
    ctx: &IngestionContext,
    cursor: &str,
) -> Result<FetchResult, ps_core::Error> {
    if ctx.person_request()?.is_some() {
        return super::person::fetch_batch(ctx, cursor).await;
    }

    let mut cur: Cursor = serde_json::from_str(cursor)
        .map_err(|e| ps_core::Error::Internal(format!("invalid cursor: {e}")))?;

    let client = DiscourseClient::new(
        ctx.http_client.clone(),
        &cur.base_url,
        &decrypt_api_key(ctx),
        &decrypt_api_username(ctx),
    );
    let fetch_likes = ctx
        .source_config
        .settings
        .get("fetch_likes")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let items = match fetch_step(ctx, &client, &mut cur, fetch_likes).await {
        Err(ps_core::Error::RateLimit { retry_after_secs }) => {
            return Ok(rate_limit_result(cursor, retry_after_secs));
        }
        result => result?,
    };

    let complete =
        cur.listing_complete && cur.pending_topics.is_empty() && cur.pending_likes.is_empty();
    if complete && cur.failed_items.is_empty() {
        cur.completed_max_bumped_at = cur.max_bumped_at.clone();
    }

    let state = serialise_cursor(&cur)?;
    Ok(FetchResult {
        items,
        next_cursor: (!complete).then(|| state.clone()),
        etag: Some(state),
        rate_limit: None,
        display_rate_limit: None,
        skipped_diffs: vec![],
    })
}

/// Each batch journals one successful HTTP operation before attempting another.
async fn fetch_step(
    ctx: &IngestionContext,
    client: &DiscourseClient,
    cur: &mut Cursor,
    fetch_likes: bool,
) -> Result<Vec<ContributionInput>, ps_core::Error> {
    if !cur.categories_loaded && cur.category_map.is_empty() {
        cur.category_map = match client.categories().await {
            Ok(categories) => categories.into_iter().map(|c| (c.id, c.name)).collect(),
            Err(error @ ps_core::Error::RateLimit { .. }) => return Err(error),
            Err(error) => {
                warn!(%error, "category names unavailable; continuing without names");
                std::collections::HashMap::new()
            }
        };
        cur.categories_loaded = true;
        return Ok(vec![]);
    }

    if let Some(post) = cur.pending_likes.front() {
        let topic = cur
            .pending_topic
            .as_ref()
            .ok_or_else(|| ps_core::Error::Internal("missing pending like topic".into()))?;
        let context = ContributionContext {
            base_url: &cur.base_url,
            instance: &cur.instance,
        };

        let likers = retry_transient(
            &format!("post_likers:{}", post.id),
            ps_core::Error::is_transient,
            || client.post_likers(post.id),
        )
        .await?;
        let items = likers
            .iter()
            .map(|liker| build_like_input(liker, post, topic, &context))
            .collect();

        cur.pending_likes.pop_front();
        if cur.pending_likes.is_empty() {
            cur.pending_topic = None;
        }
        return Ok(items);
    }

    if let Some(topic) = cur.pending_topics.front().cloned() {
        let detail = retry_transient(
            &format!("topic:{}", topic.id),
            ps_core::Error::is_transient,
            || client.topic(topic.id),
        )
        .await?;

        let context = ContributionContext {
            base_url: &cur.base_url,
            instance: &cur.instance,
        };
        let mut items = vec![];
        let category_name = topic
            .category_id
            .and_then(|id| cur.category_map.get(&id))
            .map(String::as_str);

        let mut topic_input = build_topic_input(&topic, &context, category_name);

        if let Some(ref post_stream) = detail.post_stream {
            if let Some(first_post) = post_stream.posts.iter().find(|p| p.post_number == 1) {
                topic_input.platform_username = first_post.username.to_lowercase().into();
                topic_input.content.clone_from(&first_post.raw);
                if let Some(metrics) = topic_input.metrics.as_object_mut() {
                    metrics.insert("likes".into(), first_post.likes().into());
                }

                if let Some(ref raw) = first_post.raw {
                    topic_input.enrichment_content = Some(serde_json::json!({
                        "title": topic.title,
                        "category": category_name.unwrap_or(""),
                        "tags": topic.tags,
                        "body": raw,
                    }));
                }
            }

            for post in post_stream.posts.iter().filter(|p| p.post_number != 1) {
                items.push(build_post_input(post, &topic, &context));
            }

            if fetch_likes {
                cur.pending_likes = post_stream
                    .posts
                    .iter()
                    .filter(|post| post.likes() > 0)
                    .map(super::pending::LikePost::from)
                    .collect();
            }
        }

        items.push(topic_input);

        cur.pending_topics.pop_front();
        if !cur.pending_likes.is_empty() {
            cur.pending_topic = Some(topic);
        }
        return Ok(items);
    }

    if cur.listing_complete {
        return Ok(vec![]);
    }

    let Some(response) = fetch_topic_listing(ctx, client, cur).await? else {
        advance_category(cur);
        return Ok(vec![]);
    };

    let (topics, reached_watermark) = filter_topics(&response.topic_list.topics, cur);
    cur.pending_topics = topics.into_iter().cloned().collect();
    let has_more = response.topic_list.more_topics_url.is_some();
    if response.topic_list.topics.is_empty() || reached_watermark || !has_more {
        advance_category(cur);
    } else if cur.page >= MAX_PAGES_PER_RUN {
        cur.failed_items.push(FailedItem {
            key: "pagination".into(),
            error: "Discourse page limit reached before coverage completed".into(),
        });
        advance_category(cur);
    } else {
        cur.page += 1;
        cur.has_more = true;
    }
    Ok(vec![])
}

fn advance_category(cur: &mut Cursor) {
    if cur.category_index + 1 < cur.category_ids.len() {
        cur.category_index += 1;
        cur.page = 0;
    } else {
        cur.listing_complete = true;
        cur.has_more = false;
    }
}

fn rate_limit_result(cursor: &str, seconds: u64) -> FetchResult {
    FetchResult {
        items: vec![],
        next_cursor: Some(cursor.into()),
        rate_limit: Some(RateLimitInfo {
            remaining: 0,
            limit: 0,
            reset_at: time::OffsetDateTime::now_utc()
                + time::Duration::seconds(i64::try_from(seconds).unwrap_or(86400).min(86400)),
        }),
        display_rate_limit: None,
        etag: Some(cursor.into()),
        skipped_diffs: vec![],
    }
}

/// Fetch the topic listing from either global latest or per-category endpoint.
/// Returns `None` when all categories are exhausted or a category-level error
/// was recorded. The caller advances past an exhausted or failed category.
async fn fetch_topic_listing(
    ctx: &IngestionContext,
    client: &DiscourseClient,
    cur: &mut Cursor,
) -> Result<Option<LatestResponse>, ps_core::Error> {
    if cur.category_ids.is_empty() {
        let page = cur.page;
        retry_transient("discourse latest", ps_core::Error::is_transient, || {
            client.latest(page)
        })
        .await
        .map(Some)
    } else {
        let Some(&cat_id) = cur.category_ids.get(cur.category_index) else {
            return Ok(None);
        };
        let page = cur.page;
        match retry_transient(
            &format!("category:{cat_id}"),
            ps_core::Error::is_transient,
            || client.latest_for_category(cat_id, page),
        )
        .await
        {
            Ok(r) => Ok(Some(r)),
            Err(error @ ps_core::Error::RateLimit { .. }) => Err(error),
            Err(e) => {
                warn!(
                    source = ctx.source_config.name,
                    category_id = cat_id,
                    error = %e,
                    "skipping category due to fetch error"
                );
                cur.failed_items.push(FailedItem {
                    key: format!("category:{cat_id}"),
                    error: e.to_string(),
                });
                Ok(None)
            }
        }
    }
}

/// Filter topics by watermark, category, and min-posts, updating the cursor's
/// `max_bumped_at` along the way. Returns `(filtered_topics, reached_watermark)`.
fn filter_topics<'a>(
    topics: &'a [TopicSummary],
    cur: &mut Cursor,
) -> (Vec<&'a TopicSummary>, bool) {
    let mut filtered = Vec::new();
    for topic in topics {
        // Pinned topics appear at the top regardless of bumped_at. Skip old
        // pinned topics but don't treat them as the watermark boundary.
        let bumped_at = topic.bumped_at.as_deref().or(Some(&topic.created_at));
        let older_than_watermark = matches!(
            (&cur.watermark, bumped_at),
            (Some(wm), Some(bumped)) if bumped <= wm.as_str()
        );
        if older_than_watermark {
            if topic.pinned {
                continue;
            }
            return (filtered, true);
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
        if let Some(bumped) = bumped_at
            && cur
                .max_bumped_at
                .as_ref()
                .is_none_or(|current| bumped > current.as_str())
        {
            cur.max_bumped_at = Some(bumped.to_string());
        }

        filtered.push(topic);
    }
    (filtered, false)
}
