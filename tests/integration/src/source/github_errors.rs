use super::{context, mount_search, pr};
use crate::common::wiremock_helpers::{SourceTestContext, graphql_search_response};
use ps_core::ingestion::Source;
use wiremock::matchers::method;
use wiremock::{Mock, ResponseTemplate};

#[tokio::test]
async fn person_search_and_review_errors_never_checkpoint_provider_body() {
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

    let sentinel = "provider-private-sentinel";
    let mut malformed = graphql_search_response(&[], false, None);
    malformed["data"]["search"]["issueCount"] = sentinel.into();
    let responses = [
        ResponseTemplate::new(403).set_body_string(sentinel),
        ResponseTemplate::new(200)
            .set_body_json(serde_json::json!({"errors": [{"message": sentinel}]})),
        ResponseTemplate::new(200).set_body_json(malformed),
    ];
    for cursor in [&search_cursor, &review_cursor] {
        for (index, response) in responses.iter().enumerate() {
            ctx.mock_server.reset().await;
            Mock::given(method("POST"))
                .respond_with(response.clone())
                .mount(&ctx.mock_server)
                .await;
            let result = source.fetch_batch(&ing, cursor).await.unwrap();
            assert!(result.items.is_empty());
            let persisted = result.next_cursor.unwrap();
            assert!(
                !persisted.contains(sentinel),
                "provider diagnostics must never enter durable state"
            );
            let value: serde_json::Value = serde_json::from_str(&persisted).unwrap();
            assert_eq!(value["failed_items"].as_array().unwrap().len(), 1);
            let summary = value["failed_items"][0]["error"].as_str().unwrap();
            assert!(summary.starts_with("GitHub "));
            assert!(summary.contains("permissions") || summary.contains("invalid response"));
            if index == 0 {
                assert!(summary.contains("403"));
            }
        }
    }
    ctx.teardown().await;
}
