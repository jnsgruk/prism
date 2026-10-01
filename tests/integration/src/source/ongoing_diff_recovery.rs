//! Supplementary coverage includes completion of deferred enrichment context.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
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
        SourceTestContext, github_pr_files_response, graphql_pr_node, graphql_search_response,
    },
};

#[tokio::test]
async fn supplementary_deferred_diff_failure_withholds_coverage_until_fresh_run_repairs_same_id() {
    let ctx = SourceTestContext::new().await;
    let ingestion = ctx.build_ingestion_ctx("github", Platform::Github, serde_json::json!({"base_url":ctx.mock_server.uri(), "orgs":["testorg"], "exclude_archived":false}), None, None, None).await;
    let identity = person(&ctx, Platform::Github, "manual", None).await;
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
    let recovered = Arc::new(AtomicBool::new(false));
    let diff_requests = Arc::new(AtomicUsize::new(0));
    let provider_recovered = recovered.clone();
    let provider_requests = diff_requests.clone();
    Mock::given(method("GET"))
        .and(path("/orgs/testorg/repos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/testorg/project/pulls/7/files"))
        .respond_with(move |_: &Request| {
            provider_requests.fetch_add(1, Ordering::SeqCst);
            if !provider_recovered.load(Ordering::SeqCst) {
                return ResponseTemplate::new(429)
                    .insert_header("x-ratelimit-limit", "5000")
                    .insert_header("x-ratelimit-remaining", "0")
                    .insert_header(
                        "x-ratelimit-reset",
                        (OffsetDateTime::now_utc() + Duration::seconds(1))
                            .unix_timestamp()
                            .to_string(),
                    );
            }
            ResponseTemplate::new(200)
                .insert_header("x-ratelimit-limit", "5000")
                .insert_header("x-ratelimit-remaining", "4900")
                .set_body_json(github_pr_files_response(&[(
                    "file.rs",
                    "+let complete = true;",
                )]))
        })
        .mount(&ctx.mock_server)
        .await;
    let event =
        timestamp(OffsetDateTime::now_utc().replace_nanosecond(0).unwrap() - Duration::hours(1));
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let query = body["variables"]["query"].as_str().unwrap();
            if !query.contains("created:") || query.contains("reviewed-by:") {
                return ResponseTemplate::new(200).set_body_json(graphql_search_response(
                    &[],
                    false,
                    None,
                ));
            }
            let authored = graphql_pr_node(
                "testorg",
                "project",
                7,
                "manual",
                "Deferred diff",
                "OPEN",
                &event,
                &event,
                60,
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
    let runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let failed = runtime.send_github_coordinator("github").await;
    assert!(
        !runtime.attach(&failed).await.status().is_success(),
        "deferred repair failure must fail the source invocation"
    );
    assert!(
        diff_requests.load(Ordering::SeqCst) >= 2,
        "original diff and deferred repair must both run"
    );
    let partial = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(
            &Platform::Github.to_string(),
            &["testorg/project/pull/7".into()],
        )
        .await
        .unwrap();
    assert_eq!(
        partial.len(),
        1,
        "contribution store survives failed context repair"
    );
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
    assert!(
        ctx.repos
            .activity
            .list_runs(Some("github"), Some("GithubIngestionHandler"), false)
            .await
            .unwrap()
            .iter()
            .any(|run| matches!(
                run.status,
                IngestionStatus::Failed | IngestionStatus::CompletedWithWarnings
            ))
    );
    recovered.store(true, Ordering::SeqCst);
    let retry = runtime.send_github_coordinator("github").await;
    assert!(runtime.attach(&retry).await.status().is_success());
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
            .is_some()
    );
    let repaired = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(
            &Platform::Github.to_string(),
            &["testorg/project/pull/7".into()],
        )
        .await
        .unwrap();
    assert_eq!(partial, repaired);
    assert_eq!(
        sqlx::query_scalar!("SELECT count(*) FROM activity.contributions")
            .fetch_one(&ctx.pool)
            .await
            .unwrap(),
        Some(1)
    );
    let enrichment = sqlx::query_scalar!(
        "SELECT content FROM reasoning.enrichment_queue WHERE contribution_id=$1",
        repaired[0].0
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(enrichment.to_string().contains("let complete = true"));
    drop(runtime);
    ctx.teardown().await;
}
