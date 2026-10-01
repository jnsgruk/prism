use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ps_core::{ingestion::Source, models::Platform};
use ps_workers::{
    features::ingestion::lib::discovery::identity_version, infra::registry::create_source,
};
use wiremock::{Mock, Request, ResponseTemplate, matchers::path};

use super::ongoing_tracking::{drive, person};
use crate::common::wiremock_helpers::*;

#[tokio::test]
async fn ordinary_discovery_advances_through_multiple_saved_accounts() {
    let ctx = SourceTestContext::new().await;
    let platform = Platform::Discourse("ubuntu".into());
    let ingestion = ctx
        .build_ingestion_ctx(
            "forum",
            platform.clone(),
            serde_json::json!({"base_url":ctx.mock_server.uri()}),
            None,
            None,
            None,
        )
        .await;
    let mut identities = Vec::new();
    for username in ["alice", "bob", "carol"] {
        identities.push(person(&ctx, platform.clone(), username, None).await);
    }
    Mock::given(path("/categories.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(discourse_categories_response(&[(1, "General", "general")])),
        )
        .mount(&ctx.mock_server)
        .await;
    Mock::given(path("/latest.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(discourse_latest_response(&[], false)),
        )
        .mount(&ctx.mock_server)
        .await;
    Mock::given(path("/user_actions.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"user_actions":[]})),
        )
        .expect(3)
        .mount(&ctx.mock_server)
        .await;
    let source = create_source(&platform).unwrap();
    let plan = source.plan(&ingestion).await.unwrap();
    let final_cursor = drive(
        &ingestion,
        source.as_ref(),
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
    let final_state: serde_json::Value = serde_json::from_str(&final_cursor).unwrap();
    assert_eq!(final_state["target_index"], 3);
    // PostgreSQL stores microseconds, while the frozen plan retains nanoseconds.
    let expected_cutoff = plan.discovery_cutoff.map(|cutoff| {
        cutoff
            .replace_nanosecond(cutoff.nanosecond() / 1000 * 1000)
            .unwrap()
    });
    for identity in identities {
        assert_eq!(
            ctx.repos
                .activity
                .identity_discovery_cutoff(
                    ingestion.source_config.id.into_inner(),
                    identity.identity_id,
                    &identity_version(&ingestion, &identity)
                )
                .await
                .unwrap(),
            expected_cutoff
        );
    }
    let requests = ctx.mock_server.received_requests().await.unwrap();
    let usernames: std::collections::BTreeSet<_> = requests
        .iter()
        .filter(|r| r.url.path() == "/user_actions.json")
        .flat_map(|r| {
            r.url
                .query_pairs()
                .filter(|(key, _)| key == "username")
                .map(|(_, value)| value.into_owned())
        })
        .collect();
    assert_eq!(
        usernames,
        ["alice", "bob", "carol"].map(str::to_owned).into()
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn global_discourse_rate_limits_retry_the_whole_page_without_advancing_coverage() {
    for endpoint in [
        "/categories.json",
        "/latest.json",
        "/c/1/l/latest.json",
        "/t/101.json",
        "/post_action_users.json",
    ] {
        let ctx = SourceTestContext::new().await;
        let categories: Vec<i64> = if endpoint == "/c/1/l/latest.json" {
            vec![1]
        } else {
            vec![]
        };
        let settings = serde_json::json!({
            "base_url": ctx.mock_server.uri(),
            "categories": categories,
            "fetch_likes": true,
        });
        let ingestion = ctx
            .build_ingestion_ctx(
                "forum",
                Platform::Discourse("ubuntu".into()),
                settings,
                None,
                None,
                None,
            )
            .await;
        let watermark = "2025-02-01T00:00:00Z";
        ctx.repos
            .activity
            .upsert_watermark("forum", watermark, 0)
            .await
            .unwrap();
        let topic = discourse_topic_summary(
            101,
            "Topic",
            "topic",
            Some(1),
            1,
            "2025-03-01T00:00:00Z",
            "2025-03-15T00:00:00Z",
        );
        let mut post = discourse_post(1001, 101, "alice", 1, "2025-03-01T00:00:00Z", "Body");
        post["like_count"] = 1.into();
        let responses = [
            (
                "/categories.json",
                discourse_categories_response(&[(1, "General", "general")]),
            ),
            (
                "/latest.json",
                discourse_latest_response(&[topic.clone()], false),
            ),
            (
                "/c/1/l/latest.json",
                discourse_latest_response(&[topic], false),
            ),
            (
                "/t/101.json",
                discourse_topic_detail(101, "Topic", "topic", &[post]),
            ),
            (
                "/post_action_users.json",
                serde_json::json!({"post_action_users":[{"id":1,"username":"bob"}]}),
            ),
        ];
        let limited = Arc::new(AtomicBool::new(false));
        for (route, body) in responses {
            let limited = limited.clone();
            Mock::given(path(route))
                .respond_with(move |_: &Request| {
                    if route == endpoint && !limited.swap(true, Ordering::SeqCst) {
                        ResponseTemplate::new(429).insert_header("retry-after", "2")
                    } else {
                        ResponseTemplate::new(200).set_body_json(&body)
                    }
                })
                .mount(&ctx.mock_server)
                .await;
        }
        let source = ps_workers::features::ingestion::discourse::source::DiscourseSource;
        let plan = source.plan(&ingestion).await.unwrap();
        let cursor = source.initial_cursor(&ingestion, &plan);
        let paused = source.fetch_batch(&ingestion, &cursor).await.unwrap();
        assert!(
            paused.items.is_empty(),
            "{endpoint}: no partial page may commit"
        );
        assert_eq!(paused.next_cursor.as_deref(), Some(cursor.as_str()));
        assert_eq!(paused.etag.as_deref(), Some(cursor.as_str()));
        assert_eq!(paused.rate_limit.unwrap().remaining, 0);
        assert_eq!(
            ctx.repos
                .activity
                .get_watermark("forum")
                .await
                .unwrap()
                .as_deref(),
            Some(watermark)
        );
        let recovered = source
            .fetch_batch(&ingestion, paused.next_cursor.as_deref().unwrap())
            .await
            .unwrap();
        assert_eq!(
            recovered.items.len(),
            2,
            "{endpoint}: topic and like both recovered"
        );
        assert!(recovered.next_cursor.is_none());
        assert!(recovered.rate_limit.is_none());
        source
            .store_batch(&ingestion, &recovered.items)
            .await
            .unwrap();
        let state: serde_json::Value =
            serde_json::from_str(recovered.etag.as_deref().unwrap()).unwrap();
        assert_eq!(state["max_bumped_at"], "2025-03-15T00:00:00Z");
        ctx.teardown().await;
    }
}

#[tokio::test]
async fn global_discourse_detail_errors_do_not_silently_complete_the_page() {
    let ctx = SourceTestContext::new().await;
    let ingestion = ctx
        .build_ingestion_ctx(
            "forum",
            Platform::Discourse("ubuntu".into()),
            serde_json::json!({"base_url": ctx.mock_server.uri()}),
            None,
            None,
            None,
        )
        .await;
    let topic = discourse_topic_summary(
        101,
        "Topic",
        "topic",
        Some(1),
        1,
        "2025-03-01T00:00:00Z",
        "2025-03-15T00:00:00Z",
    );
    Mock::given(path("/categories.json"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(discourse_categories_response(&[(1, "General", "general")])),
        )
        .mount(&ctx.mock_server)
        .await;
    Mock::given(path("/latest.json"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(discourse_latest_response(&[topic], false)),
        )
        .mount(&ctx.mock_server)
        .await;
    Mock::given(path("/t/101.json"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&ctx.mock_server)
        .await;
    let source = ps_workers::features::ingestion::discourse::source::DiscourseSource;
    let cursor = serde_json::json!({
        "watermark": "2025-02-01T00:00:00Z", "max_bumped_at": "2025-02-01T00:00:00Z",
        "page": 0, "category_ids": [], "category_index": 0, "min_posts": 0,
        "base_url": ctx.mock_server.uri(), "instance": "ubuntu", "has_more": true,
        "category_map": {}, "failed_items": [],
    })
    .to_string();
    assert!(matches!(
        source.fetch_batch(&ingestion, &cursor).await,
        Err(ps_core::Error::HttpStatus { status: 403, .. })
    ));
    ctx.teardown().await;
}
