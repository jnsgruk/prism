use crate::common::fixtures::create_person_with_identity;
use crate::common::wiremock_helpers::*;
use ps_core::ingestion::Source;
use ps_core::models::{ContributionType, Platform};
use wiremock::matchers::{method, path};
use wiremock::{Mock, ResponseTemplate};

fn discourse_source() -> ps_workers::features::ingestion::discourse::source::DiscourseSource {
    ps_workers::features::ingestion::discourse::source::DiscourseSource
}

fn discourse_settings(mock_uri: &str) -> serde_json::Value {
    serde_json::json!({
        "base_url": mock_uri,
        "categories": [],
        "min_posts": 0,
        "fetch_likes": false,
    })
}

fn discourse_settings_with_categories(mock_uri: &str, categories: &[i64]) -> serde_json::Value {
    serde_json::json!({
        "base_url": mock_uri,
        "categories": categories,
        "min_posts": 0,
        "fetch_likes": false,
    })
}

/// Build a Discourse cursor JSON string for fetch_batch.
fn discourse_cursor(
    mock_uri: &str,
    instance: &str,
    watermark: Option<&str>,
    category_ids: &[i64],
) -> String {
    serde_json::json!({
        "watermark": watermark,
        "page": 0,
        "category_ids": category_ids,
        "category_index": 0,
        "min_posts": 0,
        "base_url": mock_uri,
        "instance": instance,
        "max_bumped_at": watermark,
        "has_more": true,
        "category_map": {},
        "failed_items": [],
    })
    .to_string()
}

// ---------------------------------------------------------------------------
// fetch_batch tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn fetch_batch_parses_topics() {
    let ctx = SourceTestContext::new().await;

    let settings = discourse_settings(&ctx.mock_server.uri());
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "discourse-ubuntu",
            Platform::Discourse("ubuntu".into()),
            settings,
            Some("test-api-key".into()),
            None,
            Some("system".into()),
        )
        .await;

    // Mock categories endpoint
    let cats = discourse_categories_response(&[(1, "General", "general")]);
    Mock::given(method("GET"))
        .and(path("/categories.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&cats))
        .mount(&ctx.mock_server)
        .await;

    // Mock latest endpoint
    let topic = discourse_topic_summary(
        101,
        "Test Topic",
        "test-topic",
        Some(1),
        3,
        "2025-03-01T10:00:00Z",
        "2025-03-15T10:00:00Z",
    );
    let latest = discourse_latest_response(&[topic], false);

    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&latest))
        .mount(&ctx.mock_server)
        .await;

    // Mock topic detail
    let post1 = discourse_post(
        1001,
        101,
        "alice",
        1,
        "2025-03-01T10:00:00Z",
        "First post body",
    );
    let post2 = discourse_post(
        1002,
        101,
        "bob",
        2,
        "2025-03-02T10:00:00Z",
        "Reply to topic",
    );
    let detail = discourse_topic_detail(101, "Test Topic", "test-topic", &[post1, post2]);

    Mock::given(method("GET"))
        .and(path("/t/101.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&detail))
        .mount(&ctx.mock_server)
        .await;

    let source = discourse_source();
    let cursor = discourse_cursor(&ctx.mock_server.uri(), "ubuntu", None, &[]);

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch_batch");

    // Should have: 1 topic + 1 post (post_number 1 is merged into topic)
    assert_eq!(result.items.len(), 2);

    let topic_item = result
        .items
        .iter()
        .find(|i| i.contribution_type == ContributionType::DiscourseTopic)
        .expect("topic item");
    assert_eq!(topic_item.platform_id.as_str(), "101");
    assert_eq!(topic_item.title.as_deref(), Some("Test Topic"));
    // First post content merged into topic
    assert_eq!(topic_item.content.as_deref(), Some("First post body"));
    assert_eq!(topic_item.platform_username.as_str(), "alice");

    let post_item = result
        .items
        .iter()
        .find(|i| i.contribution_type == ContributionType::DiscoursePost)
        .expect("post item");
    assert_eq!(post_item.platform_id.as_str(), "1002");
    assert_eq!(post_item.platform_username.as_str(), "bob");

    ctx.teardown().await;
}

#[tokio::test]
async fn fetch_batch_respects_watermark() {
    let ctx = SourceTestContext::new().await;

    let settings = discourse_settings(&ctx.mock_server.uri());
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "discourse-ubuntu",
            Platform::Discourse("ubuntu".into()),
            settings,
            Some("test-api-key".into()),
            None,
            Some("system".into()),
        )
        .await;

    // Topic older than watermark
    let old_topic = discourse_topic_summary(
        200,
        "Old Topic",
        "old-topic",
        None,
        2,
        "2025-01-01T10:00:00Z",
        "2025-01-15T10:00:00Z",
    );
    let latest = discourse_latest_response(&[old_topic], false);

    Mock::given(method("GET"))
        .and(path("/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&latest))
        .mount(&ctx.mock_server)
        .await;

    let source = discourse_source();
    // Watermark is after the topic's bumped_at — topic should be filtered out
    let cursor = discourse_cursor(
        &ctx.mock_server.uri(),
        "ubuntu",
        Some("2025-02-01T00:00:00Z"),
        &[],
    );

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch_batch");
    assert!(
        result.items.is_empty(),
        "topic older than watermark should be filtered"
    );
    assert!(
        result.next_cursor.is_none(),
        "should stop after hitting watermark"
    );

    ctx.teardown().await;
}

#[tokio::test]
async fn category_iteration() {
    let ctx = SourceTestContext::new().await;

    let settings = discourse_settings_with_categories(&ctx.mock_server.uri(), &[1, 2]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "discourse-ubuntu",
            Platform::Discourse("ubuntu".into()),
            settings,
            Some("test-api-key".into()),
            None,
            Some("system".into()),
        )
        .await;

    // First category — empty (no topics)
    let empty = discourse_latest_response(&[], false);
    Mock::given(method("GET"))
        .and(path("/c/1/l/latest.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(&empty))
        .mount(&ctx.mock_server)
        .await;

    let source = discourse_source();
    let cursor = discourse_cursor(&ctx.mock_server.uri(), "ubuntu", None, &[1, 2]);

    let result = source
        .fetch_batch(&ing_ctx, &cursor)
        .await
        .expect("fetch_batch");
    // Empty category returns no items, but should advance to next category
    assert!(result.items.is_empty());
    // After empty category, should move to category_index=1
    if let Some(ref next) = result.next_cursor {
        let next_cur: serde_json::Value = serde_json::from_str(next).unwrap();
        assert_eq!(next_cur["category_index"], 1);
    }

    ctx.teardown().await;
}

#[tokio::test]
async fn watermark_uses_bumped_at() {
    let _ctx = SourceTestContext::new().await;

    let source = discourse_source();
    assert_eq!(
        source.watermark_field(),
        ps_core::models::WatermarkField::MaxBumpedAt
    );

    _ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// store_batch tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn store_batch_creates_contributions() {
    let ctx = SourceTestContext::new().await;

    let settings = discourse_settings(&ctx.mock_server.uri());
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "discourse-ubuntu",
            Platform::Discourse("ubuntu".into()),
            settings,
            Some("test-api-key".into()),
            None,
            Some("system".into()),
        )
        .await;

    // Create person with Discourse identity
    create_person_with_identity(
        &ctx.pool,
        "Discourse User",
        &Platform::Discourse("ubuntu".into()),
        "alice",
    )
    .await;

    let items = vec![ps_core::ingestion::ContributionInput {
        platform: Platform::Discourse("ubuntu".into()),
        contribution_type: ContributionType::DiscourseTopic,
        platform_id: "101".into(),
        platform_username: "alice".into(),
        title: Some("Test Topic".into()),
        url: Some(format!("{}/t/test-topic/101", ctx.mock_server.uri())),
        state: None,
        created_at: time::OffsetDateTime::now_utc(),
        updated_at: None,
        closed_at: None,
        metrics: serde_json::json!({"post_count": 3}),
        metadata: serde_json::json!({}),
        content: Some("Topic body".into()),
        state_history: None,
        enrichment_content: None,
    }];

    let source = discourse_source();
    let stored = source
        .store_batch(&ing_ctx, &items)
        .await
        .expect("store_batch");
    assert_eq!(stored, 1);

    let count: (i64,) = sqlx::query_as(
        "SELECT COUNT(*)::bigint FROM activity.contributions WHERE platform_id = '101'",
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(count.0, 1);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// watermark & plan tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn advance_watermark_stores_value() {
    let ctx = SourceTestContext::new().await;

    let settings = discourse_settings(&ctx.mock_server.uri());
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "discourse-ubuntu",
            Platform::Discourse("ubuntu".into()),
            settings,
            Some("test-api-key".into()),
            None,
            Some("system".into()),
        )
        .await;

    let source = discourse_source();
    source
        .advance_watermark(&ing_ctx, "2025-03-15T10:00:00Z", 20)
        .await
        .expect("advance_watermark");

    let wm = ctx
        .repos
        .activity
        .get_watermark("discourse-ubuntu")
        .await
        .unwrap();
    assert_eq!(wm.as_deref(), Some("2025-03-15T10:00:00Z"));

    ctx.teardown().await;
}

#[tokio::test]
async fn plan_defaults_watermark() {
    let ctx = SourceTestContext::new().await;

    let settings = discourse_settings(&ctx.mock_server.uri());
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "discourse-ubuntu",
            Platform::Discourse("ubuntu".into()),
            settings,
            Some("test-api-key".into()),
            None,
            Some("system".into()),
        )
        .await;

    let source = discourse_source();
    let plan = source.plan(&ing_ctx).await.expect("plan");
    assert_eq!(plan.source_name, "discourse-ubuntu");
    // Should have a default watermark (30 days ago)
    assert!(plan.watermark.is_some());

    ctx.teardown().await;
}

#[tokio::test]
async fn plan_with_existing_watermark() {
    let ctx = SourceTestContext::new().await;

    let settings = discourse_settings(&ctx.mock_server.uri());
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "discourse-ubuntu",
            Platform::Discourse("ubuntu".into()),
            settings,
            Some("test-api-key".into()),
            None,
            Some("system".into()),
        )
        .await;

    ctx.repos
        .activity
        .upsert_watermark("discourse-ubuntu", "2025-03-01T00:00:00Z", 200)
        .await
        .unwrap();

    let source = discourse_source();
    let plan = source.plan(&ing_ctx).await.expect("plan");
    assert_eq!(plan.watermark.as_deref(), Some("2025-03-01T00:00:00Z"));

    ctx.teardown().await;
}

#[tokio::test]
async fn plan_with_categories() {
    let ctx = SourceTestContext::new().await;

    let settings = discourse_settings_with_categories(&ctx.mock_server.uri(), &[5, 10]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "discourse-ubuntu",
            Platform::Discourse("ubuntu".into()),
            settings,
            Some("test-api-key".into()),
            None,
            Some("system".into()),
        )
        .await;

    let source = discourse_source();
    let plan = source.plan(&ing_ctx).await.expect("plan");
    // Categories should appear as items in the plan
    assert_eq!(plan.items.len(), 2);
    assert_eq!(plan.items[0], "5");
    assert_eq!(plan.items[1], "10");

    ctx.teardown().await;
}
