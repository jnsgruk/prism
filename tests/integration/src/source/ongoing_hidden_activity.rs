use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ps_core::models::Platform;
use ps_workers::{
    features::ingestion::lib::{discovery::identity_version, finalise::extract_failed_items},
    infra::registry::create_source,
};
use time::{Duration, OffsetDateTime};
use wiremock::{Mock, Request, ResponseTemplate, matchers::path};

use super::ongoing_tracking::{drive, person, timestamp};
use crate::common::wiremock_helpers::*;

#[tokio::test]
async fn hidden_discourse_account_does_not_stop_later_accounts_or_claim_complete_coverage() {
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
    // Discovery sorts usernames: the unavailable account must precede a valid one.
    let hidden = person(&ctx, platform.clone(), "a-hidden", None).await;
    let visible = person(&ctx, platform.clone(), "z-visible", None).await;
    let hidden_version = identity_version(&ingestion, &hidden);
    let visible_version = identity_version(&ingestion, &visible);
    let event = timestamp(OffsetDateTime::now_utc() - Duration::hours(1));
    let available = Arc::new(AtomicBool::new(false));
    let provider_available = available.clone();

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
        .respond_with(move |request: &Request| {
            let username = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "username")
                .unwrap()
                .1;
            if username == "a-hidden" && !provider_available.load(Ordering::SeqCst) {
                return ResponseTemplate::new(404);
            }
            let post_id = if username == "a-hidden" { 101 } else { 102 };
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"user_actions":[{
                "action_type":5, "post_id":post_id, "topic_id":1,
                "acting_username":username, "created_at":event,
            }]}))
        })
        .mount(&ctx.mock_server)
        .await;
    for (id, author) in [(101, "a-hidden"), (102, "z-visible")] {
        Mock::given(path(format!("/posts/{id}.json")))
            .respond_with(ResponseTemplate::new(200).set_body_json(discourse_post(
                id,
                1,
                author,
                2,
                &timestamp(OffsetDateTime::now_utc()),
                "Post content",
            )))
            .mount(&ctx.mock_server)
            .await;
    }
    let mut topic = discourse_topic_detail(1, "Topic", "topic", &[]);
    topic["category_id"] = 1.into();
    Mock::given(path("/t/1.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(topic))
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
    let failures = extract_failed_items(&final_cursor);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].key, "activity:a-hidden:offset:0");
    assert!(failures[0].error.contains("coverage is incomplete"));
    assert!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                hidden.identity_id,
                &hidden_version,
            )
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                visible.identity_id,
                &visible_version,
            )
            .await
            .unwrap()
            .is_some()
    );
    let requests = ctx.mock_server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .any(|request| request.url.path() == "/posts/102.json")
    );
    let (contributions, _) = ctx
        .repos
        .metrics
        .list_person_contributions(&ps_core::repo::metrics::ListPersonContributionsParams {
            person_id: visible.person_id.into_inner(),
            platform: None,
            contribution_type: None,
            state: None,
            since: None,
            until: None,
            search: None,
            page_size: 10,
            offset: 0,
            sort_field: None,
            sort_desc: false,
        })
        .await
        .unwrap();
    assert_eq!(contributions.len(), 1);
    assert_eq!(contributions[0].platform_id, "102");

    // Incomplete coverage must stay eligible for a later run when access returns.
    available.store(true, Ordering::SeqCst);
    let plan = source.plan(&ingestion).await.unwrap();
    let recovered = drive(
        &ingestion,
        source.as_ref(),
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
    assert!(extract_failed_items(&recovered).is_empty());
    assert!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                hidden.identity_id,
                &hidden_version,
            )
            .await
            .unwrap()
            .is_some()
    );
    ctx.teardown().await;
}
