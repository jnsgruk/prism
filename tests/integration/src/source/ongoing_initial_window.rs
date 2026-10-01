use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ps_core::{
    ingestion::{PipelineScope, ProcessingScope, SelectedSource, SourceRunContext},
    models::Platform,
};
use ps_workers::{
    features::ingestion::lib::discovery::identity_version, infra::registry::create_source,
};
use serde_json::Value;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;
use wiremock::{
    Mock, Request, ResponseTemplate,
    matchers::{method, path},
};

use super::ongoing_tracking::{drive, person};
use crate::common::wiremock_helpers::{
    SourceTestContext, graphql_pr_node, graphql_search_response,
};

#[tokio::test]
async fn first_incomplete_window_is_preserved_across_new_invocation_beyond_initial_lookback() {
    let ctx = SourceTestContext::new().await;
    let mut ingestion = ctx.build_ingestion_ctx("github", Platform::Github, serde_json::json!({"base_url":ctx.mock_server.uri(), "orgs":["testorg"], "exclude_archived":false}), Some("fake-token".into()), None, None).await;
    let identity = person(&ctx, Platform::Github, "manual", None).await;
    let first_upper = OffsetDateTime::parse(
        "2025-02-01T00:00:00Z",
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    ingestion.request = Some(SourceRunContext {
        pipeline_id: Uuid::now_v7(),
        scope: PipelineScope::All,
        source: SelectedSource {
            source_id: ingestion.source_config.id,
            source_name: ingestion.source_config.name.clone(),
            platform: Platform::Github,
            identity: None,
        },
        since_date: None,
        run_started_at: first_upper,
        processing: ProcessingScope::All,
    });
    let failed = Arc::new(AtomicBool::new(true));
    let provider_failed = failed.clone();
    Mock::given(method("GET"))
        .and(path("/orgs/testorg/repos"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/testorg/project/pulls/7/files"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("x-ratelimit-limit", "5000")
                .insert_header("x-ratelimit-remaining", "4900")
                .set_body_json(serde_json::json!([])),
        )
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let query = body["variables"]["query"].as_str().unwrap();
            if !query.contains("created:") {
                return ResponseTemplate::new(200).set_body_json(graphql_search_response(
                    &[],
                    false,
                    None,
                ));
            }
            if provider_failed.load(Ordering::SeqCst) {
                return ResponseTemplate::new(403);
            }
            let nodes = if query.contains("reviewed-by:") {
                vec![]
            } else {
                vec![graphql_pr_node(
                    "testorg",
                    "project",
                    7,
                    "manual",
                    "Deferred first-window activity",
                    "OPEN",
                    "2025-01-29T00:00:00Z",
                    "2025-01-29T00:00:00Z",
                    0,
                    0,
                    &[],
                )]
            };
            ResponseTemplate::new(200).set_body_json(graphql_search_response(&nodes, false, None))
        })
        .mount(&ctx.mock_server)
        .await;
    ctx.repos
        .activity
        .upsert_watermark("github", "2099-01-01T00:00:00Z", 0)
        .await
        .unwrap();
    let source = create_source(&Platform::Github).unwrap();
    let version = identity_version(&ingestion, &identity);
    let plan = source.plan(&ingestion).await.unwrap();
    assert_eq!(
        ctx.repos
            .activity
            .identity_discovery_initial_since(
                ingestion.source_config.id.into_inner(),
                identity.identity_id,
                &version
            )
            .await
            .unwrap(),
        Some(first_upper - Duration::days(7))
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
            .is_none(),
        "initial baseline is not completed coverage"
    );
    let incomplete = drive(
        &ingestion,
        source.as_ref(),
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
    assert!(
        !ps_workers::features::ingestion::lib::finalise::extract_failed_items(&incomplete)
            .is_empty()
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
    // A separate invocation two months later still owes the original window.
    failed.store(false, Ordering::SeqCst);
    let next_upper = first_upper + Duration::days(60);
    let request = ingestion.request.as_mut().unwrap();
    request.pipeline_id = Uuid::now_v7();
    request.run_started_at = next_upper;
    let plan = source.plan(&ingestion).await.unwrap();
    assert_eq!(
        ctx.repos
            .activity
            .identity_discovery_initial_since(
                ingestion.source_config.id.into_inner(),
                identity.identity_id,
                &version
            )
            .await
            .unwrap(),
        Some(first_upper - Duration::days(7))
    );
    drive(
        &ingestion,
        create_source(&Platform::Github).unwrap().as_ref(),
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
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
        Some(next_upper)
    );
    let saved = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(
            &Platform::Github.to_string(),
            &["testorg/project/pull/7".into()],
        )
        .await
        .unwrap();
    assert_eq!(
        saved.len(),
        1,
        "failed first-window activity cannot age out of coverage"
    );
    ctx.teardown().await;
}
