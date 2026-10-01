use super::{discourse_settings, discourse_source};
use crate::common::wiremock_helpers::*;
use ps_core::ingestion::{IngestionContext, Source};
use ps_core::models::{ContributionType, Platform};
use time::OffsetDateTime;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

const EVENT: &str = "2025-03-15T10:00:00Z";

async fn person_context(ctx: &SourceTestContext, likes: bool) -> IngestionContext {
    let mut settings = discourse_settings(&ctx.mock_server.uri());
    settings["fetch_likes"] = likes.into();
    let mut context = ctx
        .build_ingestion_ctx(
            "discourse-ubuntu",
            Platform::Discourse("ubuntu".into()),
            settings,
            Some("test-api-key".into()),
            None,
            Some("system".into()),
        )
        .await;
    let upper = OffsetDateTime::parse(
        "2025-04-01T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    ctx.with_person_scope(&mut context, "alice", None, "2025-03-01", upper)
        .await;
    Mock::given(path("/categories.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(discourse_categories_response(&[(1, "General", "general")])),
        )
        .mount(&ctx.mock_server)
        .await;
    context
}

fn action(_id: i64, kind: i32, post: i64, topic: i64, actor: &str) -> serde_json::Value {
    serde_json::json!({"action_type":kind,"post_id":post,"topic_id":topic,
        "acting_username":actor,"acting_name":actor,"created_at":EVENT})
}

async fn mount_feed(ctx: &SourceTestContext, actions: Vec<serde_json::Value>) {
    Mock::given(method("GET"))
        .and(path("/user_actions.json"))
        .and(query_param("username", "alice"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"user_actions":actions})),
        )
        .mount(&ctx.mock_server)
        .await;
}

async fn mount_detail(
    ctx: &SourceTestContext,
    post_id: i64,
    topic_id: i64,
    author: &str,
    number: i32,
    category: i64,
    count: i32,
) {
    let mut post = discourse_post(
        post_id,
        topic_id,
        author,
        number,
        "2020-01-01T00:00:00Z",
        "Exact post outside the topic stream",
    );
    post["actions_summary"] = serde_json::json!([{"id":2,"count":7}]);
    Mock::given(path(format!("/posts/{post_id}.json")))
        .respond_with(ResponseTemplate::new(200).set_body_json(post))
        .mount(&ctx.mock_server)
        .await;
    // No embedded posts: the adapter must retrieve the action's specific post.
    let mut topic = discourse_topic_detail(topic_id, "Old topic", "old-topic", &[]);
    topic["category_id"] = category.into();
    topic["posts_count"] = count.into();
    topic["tags"] = serde_json::json!(["engineering"]);
    Mock::given(path(format!("/t/{topic_id}.json")))
        .respond_with(ResponseTemplate::new(200).set_body_json(topic))
        .mount(&ctx.mock_server)
        .await;
}

async fn fetch_first(context: &IngestionContext) -> ps_core::ingestion::FetchResult {
    let source = discourse_source();
    let plan = source.plan(context).await.unwrap();
    assert_eq!(plan.watermark.as_deref(), Some("2025-03-01"));
    source
        .fetch_batch(context, &source.initial_cursor(context, &plan))
        .await
        .unwrap()
}

#[tokio::test]
async fn mixed_actions_exact_actor_event_time_and_specific_post_are_persisted() {
    let ctx = SourceTestContext::new().await;
    let context = person_context(&ctx, true).await;
    mount_feed(
        &ctx,
        vec![
            action(1, 4, 1001, 101, "Alice"),
            action(2, 5, 1002, 102, "alice"),
            action(3, 1, 1003, 103, "alice"),
            action(4, 2, 1004, 104, "alice"),
            action(5, 6, 1005, 105, "alice"),
            action(6, 5, 1006, 106, "bob"),
            action(7, 5, 1001, 101, "alice"),
            action(2, 5, 1002, 102, "alice"),
        ],
    )
    .await;
    mount_detail(&ctx, 1001, 101, "alice", 1, 1, 30).await;
    mount_detail(&ctx, 1002, 102, "alice", 48, 1, 90).await;
    mount_detail(&ctx, 1003, 103, "bob", 9, 1, 90).await;
    let fetched = fetch_first(&context).await;
    assert_eq!(fetched.items.len(), 3);
    let replay = fetch_first(&context).await;
    assert_eq!(fetched.etag, replay.etag);
    assert_eq!(
        serde_json::to_value(&fetched.items).unwrap(),
        serde_json::to_value(&replay.items).unwrap()
    );
    assert!(fetched.next_cursor.is_none());
    let event =
        OffsetDateTime::parse(EVENT, &time::format_description::well_known::Rfc3339).unwrap();
    for item in &fetched.items {
        assert_eq!(item.platform_username.as_str(), "alice");
        assert_eq!(item.created_at, event);
        assert_eq!(item.platform, Platform::Discourse("ubuntu".into()));
    }
    let topic = fetched
        .items
        .iter()
        .find(|i| i.contribution_type == ContributionType::DiscourseTopic)
        .unwrap();
    assert_eq!(topic.metrics["category"], "General");
    assert_eq!(topic.metrics["likes"], 7);
    assert!(topic.enrichment_content.is_some());
    let post = fetched
        .items
        .iter()
        .find(|i| i.contribution_type == ContributionType::DiscoursePost)
        .unwrap();
    assert_eq!(post.metrics["likes"], 7);
    assert!(post.url.as_ref().unwrap().ends_with("/102/48"));
    let like = fetched
        .items
        .iter()
        .find(|i| i.contribution_type == ContributionType::DiscourseLike)
        .unwrap();
    assert_eq!(like.platform_id.as_str(), "like-1003-alice");
    assert_eq!(like.metadata["post_author"], "bob");
    assert_eq!(like.metadata["event_time_source"], "discourse_user_action");
    // Seed the legacy timestamp (target post creation) under the same stable key.
    let mut legacy_like = like.clone();
    legacy_like.created_at = OffsetDateTime::parse(
        "2020-01-01T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    legacy_like.metadata = serde_json::json!({ "username": "alice", "post_author": "bob" });
    let person_id = context.request.as_ref().unwrap().scope.person_id().unwrap();
    ctx.repos
        .activity
        .upsert_contribution(
            uuid::Uuid::now_v7(),
            Some(*person_id.as_uuid()),
            &legacy_like,
        )
        .await
        .unwrap();
    assert_eq!(
        discourse_source()
            .store_batch(&context, &fetched.items)
            .await
            .unwrap(),
        3
    );
    // Real repo retrieval verifies an active person with no team can own rows.
    let (result, _) = ctx
        .repos
        .metrics
        .list_person_contributions(&ps_core::repo::metrics::ListPersonContributionsParams {
            person_id: *person_id.as_uuid(),
            platform: None,
            contribution_type: None,
            state: None,
            since: None,
            until: None,
            search: None,
            page_size: 100,
            offset: 0,
            sort_field: None,
            sort_desc: false,
        })
        .await
        .unwrap();
    assert_eq!(result.len(), 3);
    let stored_like = result
        .iter()
        .find(|i| i.platform_id == "like-1003-alice")
        .unwrap();
    assert_eq!(stored_like.created_at, event);
    ctx.teardown().await;
}

#[tokio::test]
async fn disabled_likes_and_category_minimum_policies_do_not_credit_other_authors() {
    let ctx = SourceTestContext::new().await;
    let mut context = person_context(&ctx, false).await;
    context.source_config.settings["categories"] = serde_json::json!([1]);
    context.source_config.settings["min_posts"] = 10.into();
    mount_feed(
        &ctx,
        vec![
            action(1, 1, 1001, 101, "alice"),
            action(2, 5, 1002, 102, "alice"),
            action(3, 5, 1003, 103, "alice"),
            action(4, 5, 1004, 104, "alice"),
        ],
    )
    .await;
    mount_detail(&ctx, 1002, 102, "alice", 2, 2, 100).await;
    mount_detail(&ctx, 1003, 103, "alice", 2, 1, 2).await;
    mount_detail(&ctx, 1004, 104, "alice", 50, 1, 100).await;
    let fetched = fetch_first(&context).await;
    assert_eq!(fetched.items.len(), 1);
    assert_eq!(fetched.items[0].platform_id.as_str(), "1004");
    assert!(
        ctx.mock_server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .all(|r| !r.url.path().contains("1001"))
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn feed_and_detail_rate_limits_preserve_cursor_for_durable_sleep() {
    for detail in [false, true] {
        let ctx = SourceTestContext::new().await;
        let context = person_context(&ctx, false).await;
        if detail {
            mount_feed(&ctx, vec![action(1, 5, 1001, 101, "alice")]).await;
            Mock::given(path("/posts/1001.json"))
                .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "37"))
                .mount(&ctx.mock_server)
                .await;
            mount_detail(&ctx, 1002, 101, "alice", 2, 1, 30).await;
        } else {
            Mock::given(path("/user_actions.json"))
                .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "37"))
                .mount(&ctx.mock_server)
                .await;
        }
        let first = fetch_first(&context).await;
        assert!(first.items.is_empty());
        let rate = first.rate_limit.unwrap();
        assert_eq!(rate.remaining, 0);
        assert!(rate.reset_at > OffsetDateTime::now_utc() + time::Duration::seconds(30));
        let cursor: serde_json::Value =
            serde_json::from_str(first.next_cursor.as_ref().unwrap()).unwrap();
        assert_eq!(cursor["offset"], 0);
        assert!(cursor["seen_actions"].as_array().unwrap().is_empty());
        ctx.teardown().await;
    }
}

#[tokio::test]
async fn unavailable_account_activity_records_incomplete_coverage_without_checkpointing() {
    let ctx = SourceTestContext::new().await;
    let context = person_context(&ctx, false).await;
    Mock::given(path("/user_actions.json"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&ctx.mock_server)
        .await;

    let fetched = fetch_first(&context).await;
    assert!(fetched.items.is_empty());
    assert!(fetched.next_cursor.is_none());
    assert!(fetched.rate_limit.is_none());
    let checkpoint = fetched.etag.unwrap();
    let cursor: serde_json::Value = serde_json::from_str(&checkpoint).unwrap();
    assert_eq!(cursor["discovery_complete"], true);
    assert_eq!(cursor["failed_items"][0]["key"], "activity:alice:offset:0");
    assert!(
        cursor["failed_items"][0]["error"]
            .as_str()
            .unwrap()
            .contains("coverage is incomplete")
    );

    let source =
        ps_workers::infra::registry::create_source(&context.source_config.source_type).unwrap();
    source
        .checkpoint_batch(&context, &checkpoint)
        .await
        .unwrap();
    let identity = context
        .request
        .as_ref()
        .unwrap()
        .source
        .identity
        .as_ref()
        .unwrap();
    let version =
        ps_workers::features::ingestion::lib::discovery::identity_version(&context, identity);
    assert!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                context.source_config.id.into_inner(),
                identity.identity_id,
                &version
            )
            .await
            .unwrap()
            .is_none()
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn private_profile_fails_and_inaccessible_details_report_limited_coverage() {
    let ctx = SourceTestContext::new().await;
    let context = person_context(&ctx, true).await;
    Mock::given(path("/user_actions.json"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&ctx.mock_server)
        .await;
    let source = discourse_source();
    let plan = source.plan(&context).await.unwrap();
    assert!(
        source
            .fetch_batch(&context, &source.initial_cursor(&context, &plan))
            .await
            .is_err()
    );
    ctx.mock_server.reset().await;
    mount_feed(&ctx, vec![action(1, 5, 1001, 101, "alice")]).await;
    Mock::given(path("/posts/1001.json"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&ctx.mock_server)
        .await;
    mount_detail(&ctx, 1002, 101, "alice", 2, 1, 30).await;
    let result = fetch_first(&context).await;
    assert!(result.items.is_empty());
    let cursor: serde_json::Value = serde_json::from_str(result.etag.as_ref().unwrap()).unwrap();
    assert!(!cursor["failed_items"].as_array().unwrap().is_empty());
    assert!(
        cursor["coverage"]
            .as_str()
            .unwrap()
            .contains("visible_activity_only")
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn hidden_post_and_mismatched_authorship_are_partial_failures() {
    let ctx = SourceTestContext::new().await;
    let context = person_context(&ctx, true).await;
    let mut hidden = action(1, 5, 1001, 101, "alice");
    hidden["hidden"] = true.into();
    mount_feed(
        &ctx,
        vec![
            hidden,
            action(2, 5, 1002, 102, "alice"),
            action(3, 1, 1003, 103, "alice"),
        ],
    )
    .await;
    mount_detail(&ctx, 1002, 102, "bob", 40, 1, 100).await;
    mount_detail(&ctx, 1003, 103, "bob", 40, 1, 100).await;
    let result = fetch_first(&context).await;
    assert_eq!(result.items.len(), 1);
    assert_eq!(
        result.items[0].contribution_type,
        ContributionType::DiscourseLike
    );
    let cursor: serde_json::Value = serde_json::from_str(result.etag.as_ref().unwrap()).unwrap();
    assert_eq!(cursor["failed_items"].as_array().unwrap().len(), 2);
    ctx.teardown().await;
}

#[tokio::test]
async fn official_topic_action_with_null_post_id_and_hidden_uses_initial_post() {
    let ctx = SourceTestContext::new().await;
    let context = person_context(&ctx, false).await;
    let mut topic_action = action(1, 4, 0, 101, "alice");
    topic_action["post_id"] = serde_json::Value::Null;
    topic_action["hidden"] = serde_json::Value::Null;
    mount_feed(&ctx, vec![topic_action]).await;
    let post = discourse_post(1001, 101, "alice", 1, EVENT, "Topic creation content");
    Mock::given(path("/t/101.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(discourse_topic_detail(
                101,
                "Topic",
                "topic",
                &[post],
            )),
        )
        .mount(&ctx.mock_server)
        .await;
    let result = fetch_first(&context).await;
    assert_eq!(result.items.len(), 1);
    assert_eq!(
        result.items[0].contribution_type,
        ContributionType::DiscourseTopic
    );
    assert_eq!(
        result.items[0].content.as_deref(),
        Some("Topic creation content")
    );
    assert_eq!(result.items[0].metadata["post_id"], 1001);
    ctx.teardown().await;
}

#[path = "discourse_person_history.rs"]
mod history;
