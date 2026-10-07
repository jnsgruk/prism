//! Resumable instance-bound activity traversal. Overlap detects offset shifts;
//! activity invisible to the configured API principal is explicitly limited.
use std::collections::{BTreeMap, BTreeSet, HashSet};

use futures::stream::{self, StreamExt};
use ps_core::ingestion::{FailedItem, FetchResult, IngestionContext, SourceRunContext};
use ps_core::models::RateLimitInfo;
use serde::{Deserialize, Serialize};

use super::super::client::{DiscourseClient, UserAction};
use super::inputs::parse_discourse_datetime;
use super::{decrypt_api_key, decrypt_api_username, rate_limit_wait_secs};
use crate::infra::retry::retry_transient;

const PAGE_SIZE: usize = 60;
const OVERLAP: usize = 10;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PersonCursor {
    pub request: SourceRunContext,
    pub base_url: String,
    pub category_ids: Vec<i64>,
    pub min_posts: i32,
    pub fetch_likes: bool,
    pub offset: u64,
    pub phase: String,
    pub position: usize,
    pub anchors: Vec<String>,
    pub seen_actions: BTreeSet<String>,
    pub category_map: BTreeMap<i64, String>,
    pub categories_loaded: bool,
    pub failed_items: Vec<FailedItem>,
    pub coverage: String,
    #[serde(default, skip_serializing_if = "super::rate_limit_streak_is_zero")]
    pub rate_limit_streak: u32,
}

pub(super) fn initial_cursor(ctx: &IngestionContext, request: &SourceRunContext) -> String {
    let settings = &ctx.source_config.settings;
    let cursor = PersonCursor {
        request: request.clone(),
        base_url: settings
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .trim_end_matches('/')
            .into(),
        category_ids: settings
            .get("categories")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default(),
        min_posts: settings
            .get("min_posts")
            .and_then(serde_json::Value::as_i64)
            .and_then(|v| i32::try_from(v).ok())
            .unwrap_or_default(),
        fetch_likes: settings
            .get("fetch_likes")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        offset: 0,
        phase: "authored_topics_posts_and_configured_likes".into(),
        position: 0,
        anchors: vec![],
        seen_actions: BTreeSet::new(),
        category_map: BTreeMap::new(),
        categories_loaded: false,
        failed_items: vec![],
        coverage: "visible_activity_only: Discourse omits private, deleted and hidden history unavailable to configured credentials".into(),
        rate_limit_streak: 0,
    };
    serde_json::to_string(&cursor).unwrap_or_default()
}

pub(super) async fn fetch_batch(
    ctx: &IngestionContext,
    raw: &str,
) -> Result<FetchResult, ps_core::Error> {
    let mut cursor: PersonCursor = serde_json::from_str(raw)
        .map_err(|e| ps_core::Error::Validation(format!("invalid Discourse person cursor: {e}")))?;
    if ctx.person_request()? != Some(&cursor.request) || cursor.base_url.is_empty() {
        return Err(ps_core::Error::Validation(
            "Discourse cursor differs from frozen target".into(),
        ));
    }
    let username = cursor
        .request
        .source
        .identity
        .as_ref()
        .ok_or_else(|| ps_core::Error::Validation("saved Discourse identity required".into()))?
        .username
        .to_string();
    let client = DiscourseClient::new(
        ctx.http_client.clone(),
        &cursor.base_url,
        &decrypt_api_key(ctx),
        &decrypt_api_username(ctx),
    );

    if !cursor.categories_loaded {
        match retry_transient("discourse categories", ps_core::Error::is_transient, || {
            client.categories()
        })
        .await
        {
            Ok(categories) => {
                cursor.category_map = categories.into_iter().map(|c| (c.id, c.name)).collect();
            }
            Err(ps_core::Error::RateLimit { retry_after_secs }) => {
                return result(cursor, vec![], false, Some(retry_after_secs));
            }
            Err(error) => cursor.failed_items.push(FailedItem {
                key: "categories".into(),
                error: error.to_string(),
            }),
        }
        cursor.categories_loaded = true;
    }
    let actions = match retry_transient(
        "discourse user actions",
        ps_core::Error::is_transient,
        || client.user_actions(&username, cursor.offset, PAGE_SIZE, cursor.fetch_likes),
    )
    .await
    {
        Ok(actions) => actions,
        Err(ps_core::Error::RateLimit { retry_after_secs }) => {
            return result(cursor, vec![], false, Some(retry_after_secs));
        }
        Err(error @ ps_core::Error::HttpStatus { status: 404, .. }) => {
            cursor.failed_items.push(FailedItem {
                key: format!("activity:{username}:offset:{}", cursor.offset),
                error: format!(
                    "account activity unavailable to configured credentials; coverage is incomplete: {error}"
                ),
            });
            return result(cursor, vec![], true, None);
        }
        Err(error) if cursor.offset > 0 => {
            cursor.failed_items.push(FailedItem {
                key: format!("activity_offset:{}", cursor.offset),
                error: format!("older activity unavailable; coverage is incomplete: {error}"),
            });
            return result(cursor, vec![], true, None);
        }
        Err(error) => return Err(error),
    };
    let start = match continuation_start(&actions, &cursor.anchors) {
        Ok(start) => start,
        Err(error) => {
            cursor.failed_items.push(FailedItem {
                key: format!("activity_offset:{}", cursor.offset),
                error,
            });
            return result(cursor, vec![], true, None);
        }
    };
    let (eligible, reached_lower) = match select_actions(&cursor, &actions, start) {
        Ok(selection) => selection,
        Err(error) => {
            cursor.failed_items.push(FailedItem {
                key: "activity_pagination".into(),
                error: error.to_string(),
            });
            return result(cursor, vec![], true, None);
        }
    };

    let details = stream::iter(eligible)
        .map(|action| {
            let client = &client;
            let cursor = &cursor;
            async move {
                let detail = super::person_items::fetch_item(client, cursor, &action).await;
                (action, detail)
            }
        })
        .buffered(4)
        .collect::<Vec<_>>()
        .await;
    // Rate limits retry this exact page, including already fetched details; no
    // position or dedup state is committed until the batch is complete.
    if let Some(seconds) = details
        .iter()
        .filter_map(|(_, r)| match r {
            Err(ps_core::Error::RateLimit { retry_after_secs }) => Some(*retry_after_secs),
            _ => None,
        })
        .max()
    {
        return result(cursor, vec![], false, Some(seconds));
    }
    let mut items = Vec::new();
    let mut seen_contributions = HashSet::new();
    for (action, detail) in details {
        match detail {
            Ok(Some(item)) if seen_contributions.insert(item.platform_id.to_string()) => {
                items.push(item);
            }
            Ok(_) => {}
            Err(error) => cursor.failed_items.push(FailedItem {
                key: format!(
                    "user_action:{}:post:{:?}:topic:{}",
                    action.fingerprint(),
                    action.post_id,
                    action.topic_id
                ),
                error: error.to_string(),
            }),
        }
    }
    // Bound checkpoint memory to one page. Stable contribution IDs and the
    // shared transactional store preserve idempotency across older pages.
    cursor.seen_actions = actions.iter().map(UserAction::fingerprint).collect();
    cursor.position = actions.len();
    let done = reached_lower || actions.len() < PAGE_SIZE;
    if !done {
        cursor.anchors = actions
            .iter()
            .skip(actions.len() - OVERLAP)
            .map(UserAction::fingerprint)
            .collect();
        cursor.offset += u64::try_from(actions.len() - OVERLAP)
            .map_err(|error| ps_core::Error::Internal(error.to_string()))?;
    }
    result(cursor, items, done, None)
}

fn select_actions(
    cursor: &PersonCursor,
    actions: &[UserAction],
    start: usize,
) -> Result<(Vec<UserAction>, bool), ps_core::Error> {
    let lower = ps_core::ingestion::parse_since_date(
        cursor
            .request
            .since_date
            .as_deref()
            .ok_or_else(|| ps_core::Error::Validation("backfill date required".into()))?,
    )?
    .midnight()
    .assume_utc();
    let mut eligible = Vec::new();
    let mut reached_lower = false;
    let mut previous_time = start
        .checked_sub(1)
        .and_then(|index| actions.get(index))
        .map(|action| parse_discourse_datetime(&action.created_at))
        .transpose()?;
    let mut page_seen = cursor.seen_actions.clone();
    let mut new_actions = 0;
    for action in actions.iter().skip(start) {
        let event = parse_discourse_datetime(&action.created_at)?;
        if previous_time.is_some_and(|previous| event > previous) {
            return Err(ps_core::Error::Validation(
                "activity feed is not descending; lower boundary cannot establish coverage".into(),
            ));
        }
        previous_time = Some(event);
        if event < lower {
            reached_lower = true;
            continue;
        }
        if !page_seen.insert(action.fingerprint()) {
            continue;
        }
        new_actions += 1;
        if cursor.request.contains_event(event)? {
            eligible.push(action.clone());
        }
    }
    if actions.len() >= PAGE_SIZE && new_actions == 0 && !reached_lower {
        return Err(ps_core::Error::Validation(
            "nonadvancing activity page; complete coverage cannot be proved".into(),
        ));
    }
    Ok((eligible, reached_lower))
}

fn continuation_start(actions: &[UserAction], anchors: &[String]) -> Result<usize, String> {
    if anchors.is_empty() {
        return Ok(0);
    }
    let ids: Vec<_> = actions.iter().map(UserAction::fingerprint).collect();
    ids.windows(anchors.len())
        .position(|window| window == anchors)
        .map(|start| start + anchors.len())
        .ok_or_else(|| "offset history mutated beyond safe overlap; coverage is incomplete".into())
}

fn result(
    mut cursor: PersonCursor,
    items: Vec<ps_core::ingestion::ContributionInput>,
    done: bool,
    sleep: Option<u64>,
) -> Result<FetchResult, ps_core::Error> {
    let wait_secs = if let Some(retry_after_secs) = sleep {
        cursor.rate_limit_streak = cursor.rate_limit_streak.saturating_add(1);
        Some(rate_limit_wait_secs(
            retry_after_secs,
            cursor.rate_limit_streak,
        ))
    } else {
        cursor.rate_limit_streak = 0;
        None
    };

    let mut value =
        serde_json::to_value(&cursor).map_err(|e| ps_core::Error::Internal(e.to_string()))?;
    if let Some(object) = value.as_object_mut() {
        object.insert("discovery_complete".into(), done.into());
    }
    let raw = value.to_string();
    let rate_limit = wait_secs.map(|seconds| RateLimitInfo {
        remaining: 0,
        limit: 0,
        reset_at: time::OffsetDateTime::now_utc()
            + time::Duration::seconds(i64::try_from(seconds).unwrap_or(i64::MAX / 2).min(86400)),
    });
    Ok(FetchResult {
        items,
        next_cursor: (!done).then(|| raw.clone()),
        etag: Some(raw),
        rate_limit,
        display_rate_limit: None,
        skipped_diffs: vec![],
    })
}
