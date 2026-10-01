//! Ordinary account discovery checkpoints survive actual durable sleeps/replay.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ps_core::models::{IngestionStatus, Platform, SecretKey};
use ps_workers::features::ingestion::lib::discovery::identity_version;
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;
use wiremock::{
    Mock, Request, ResponseTemplate,
    matchers::{method, path},
};

use super::ongoing_tracking::{person, timestamp};
use crate::common::{
    restate::{RestateTestContext, TEST_SECRET_KEY},
    wiremock_helpers::{
        SourceTestContext, graphql_pr_node, graphql_review_node, graphql_search_response,
    },
};

#[tokio::test]
async fn ongoing_account_discovery_restarts_during_durable_rate_sleep_and_retries_partial_targets()
{
    let ctx = SourceTestContext::new().await;
    let ingestion = ctx.build_ingestion_ctx("github", Platform::Github, serde_json::json!({"base_url":ctx.mock_server.uri(), "orgs":["testorg"], "exclude_archived":false}), None, None, None).await;
    let identity = person(&ctx, Platform::Github, "manual", None).await;
    let inactive = person(&ctx, Platform::Github, "inactive", None).await;
    ctx.repos
        .org
        .deactivate_person(inactive.person_id.into_inner())
        .await
        .unwrap();
    let token = ps_core::crypto::encrypt(&TEST_SECRET_KEY, b"fake-github-token").unwrap();
    ctx.repos
        .config
        .upsert_secret(
            Uuid::now_v7(),
            ingestion.source_config.id.into_inner(),
            SecretKey::ApiToken.as_str(),
            &token,
        )
        .await
        .unwrap();
    ctx.repos
        .activity
        .upsert_watermark("github", "2099-01-01T00:00:00Z", 0)
        .await
        .unwrap();
    let deferred = Arc::new(AtomicBool::new(false));
    let blocked = Arc::new(AtomicBool::new(false));
    let provider_deferred = deferred.clone();
    let provider_blocked = blocked.clone();
    let event =
        timestamp(OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::hours(1));
    Mock::given(method("GET"))
        .and(path("/orgs/testorg/repos"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ratelimit-remaining", "4900")
                .insert_header("x-ratelimit-limit", "5000")
                .set_body_json(serde_json::json!([])),
        )
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/testorg/project/pulls/7/files"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ratelimit-remaining", "4900")
                .insert_header("x-ratelimit-limit", "5000")
                .set_body_json(serde_json::json!([])),
        )
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let query = body["variables"]["query"].as_str().unwrap();
            assert!(!query.contains("inactive"));
            if !query.contains("created:") {
                return ResponseTemplate::new(200).set_body_json(graphql_search_response(
                    &[],
                    false,
                    None,
                ));
            }
            if query.contains("reviewed-by:") {
                if provider_blocked.load(Ordering::SeqCst) {
                    return ResponseTemplate::new(403);
                }
                if !provider_deferred.swap(true, Ordering::SeqCst) {
                    return ResponseTemplate::new(429)
                        .insert_header("x-ratelimit-limit", "5000")
                        .insert_header("x-ratelimit-remaining", "0")
                        .insert_header(
                            "x-ratelimit-reset",
                            (OffsetDateTime::now_utc().replace_nanosecond(0).unwrap()
                                + Duration::seconds(10))
                            .unix_timestamp()
                            .to_string(),
                        );
                }
                let parent = graphql_pr_node(
                    "testorg",
                    "outside",
                    42,
                    "other",
                    "Cross repo review",
                    "OPEN",
                    "2010-01-01T00:00:00Z",
                    &event,
                    0,
                    0,
                    &[graphql_review_node("manual", "APPROVED", &event, 1)],
                );
                return ResponseTemplate::new(200).set_body_json(graphql_search_response(
                    &[parent],
                    false,
                    None,
                ));
            }
            let authored = graphql_pr_node(
                "testorg",
                "project",
                7,
                "manual",
                "Authored PR",
                "OPEN",
                &event,
                &event,
                0,
                0,
                &[],
            );
            ResponseTemplate::new(200).set_body_json(graphql_search_response(
                &[authored],
                false,
                None,
            ))
        })
        .mount(&ctx.mock_server)
        .await;
    let version = identity_version(&ingestion, &identity);
    let mut runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let first = runtime.send_github_coordinator("github").await;
    for _ in 0..100 {
        if deferred.load(Ordering::SeqCst) {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(deferred.load(Ordering::SeqCst));
    // Authored data already committed while independent review coverage sleeps.
    let partial = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(
            &Platform::Github.to_string(),
            &["testorg/project/pull/7".into()],
        )
        .await
        .unwrap();
    assert_eq!(partial.len(), 1);
    assert!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                identity.identity_id,
                &version
            )
            .await
            .unwrap()
            .is_none()
    );
    runtime.restart_worker().await;
    let response = runtime.attach(&first).await;
    assert!(
        response.status().is_success(),
        "normal restart should resume durable sleep"
    );
    let completed = ctx
        .repos
        .activity
        .identity_discovery_cutoff(
            ingestion.source_config.id.into_inner(),
            identity.identity_id,
            &version,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contributions")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(2)
    );
    blocked.store(true, Ordering::SeqCst);
    let failed = runtime.send_github_coordinator("github").await;
    assert!(runtime.attach(&failed).await.status().is_success());
    assert_eq!(
        ctx.repos
            .activity
            .identity_discovery_cutoff(
                ingestion.source_config.id.into_inner(),
                identity.identity_id,
                &version
            )
            .await
            .unwrap(),
        Some(completed)
    );
    assert!(
        ctx.repos
            .activity
            .list_runs(Some("github"), Some("GithubIngestionHandler"), false)
            .await
            .unwrap()
            .iter()
            .any(|run| run.status == IngestionStatus::CompletedWithWarnings)
    );
    blocked.store(false, Ordering::SeqCst);
    let recovery = runtime.send_github_coordinator("github").await;
    assert!(runtime.attach(&recovery).await.status().is_success());
    let recovered = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(
            &Platform::Github.to_string(),
            &["testorg/project/pull/7".into()],
        )
        .await
        .unwrap();
    assert_eq!(
        partial, recovered,
        "repeated discovery preserves natural key and ID"
    );
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contributions")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(2)
    );
    drop(runtime);
    ctx.teardown().await;
}
