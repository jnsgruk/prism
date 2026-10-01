use super::{context, mount_search, pr};
use crate::common::wiremock_helpers::{SourceTestContext, graphql_search_response};
use ps_core::ingestion::Source;

#[tokio::test]
async fn search_and_pending_review_cursors_reject_changed_endpoint_or_run_before_fetch() {
    let ctx = SourceTestContext::new().await;
    let ing = context(&ctx).await;
    let source = super::super::github_source();
    let plan = source.plan(&ing).await.unwrap();
    let search_cursor = source.initial_cursor(&ing, &plan);
    let mut parent = pr("testorg", "allowed", "other", &[]);
    parent["reviews"]["pageInfo"] =
        serde_json::json!({"hasNextPage": true, "endCursor": "inline-end"});
    mount_search(&ctx, graphql_search_response(&[parent], false, None)).await;
    let review_cursor = source
        .fetch_batch(&ing, &search_cursor)
        .await
        .unwrap()
        .next_cursor
        .unwrap();
    ctx.mock_server.reset().await;

    let mut changed_endpoint = ing.clone();
    changed_endpoint.source_config.settings["base_url"] =
        format!("{}/different-instance", ctx.mock_server.uri()).into();
    let mut changed_date = ing.clone();
    changed_date.request.as_mut().unwrap().since_date = Some("2025-03-02".into());
    let mut changed_boundary = ing.clone();
    changed_boundary.request.as_mut().unwrap().run_started_at += time::Duration::days(1);
    let mut changed_pipeline = ing.clone();
    changed_pipeline.request.as_mut().unwrap().pipeline_id = uuid::Uuid::now_v7();
    for cursor in [&search_cursor, &review_cursor] {
        for changed in [
            &changed_endpoint,
            &changed_date,
            &changed_boundary,
            &changed_pipeline,
        ] {
            let error = source.fetch_batch(changed, cursor).await.unwrap_err();
            assert!(error.to_string().contains("start a new backfill"));
            assert!(
                ctx.mock_server
                    .received_requests()
                    .await
                    .unwrap()
                    .is_empty(),
                "binding checks must precede both search and review requests"
            );
        }
        let mut unbound: serde_json::Value = serde_json::from_str(cursor).unwrap();
        unbound["person"].as_object_mut().unwrap().remove("request");
        assert!(
            source
                .fetch_batch(&ing, &unbound.to_string())
                .await
                .is_err()
        );
        assert!(
            ctx.mock_server
                .received_requests()
                .await
                .unwrap()
                .is_empty()
        );
    }
    ctx.teardown().await;
}
