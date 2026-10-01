use super::{context, pr, stored_count};
use crate::common::wiremock_helpers::{
    SourceTestContext, graphql_review_node, graphql_search_response,
};
use ps_core::ingestion::Source;
use ps_core::models::Platform;
use serde_json::Value;
use wiremock::{
    Mock, Request, ResponseTemplate,
    matchers::{method, path},
};

#[tokio::test]
async fn author_search_uses_event_window_while_reviews_retain_old_parents_in_each_org() {
    let ctx = SourceTestContext::new().await;
    let ingestion = context(&ctx).await;
    let mut authored = pr("testorg", "authored", "alice", &[]);
    authored["createdAt"] = "2025-03-01T00:00:00Z".into();
    let reviewed = pr(
        "testorg",
        "old-parent",
        "other",
        &[graphql_review_node(
            "alice",
            "APPROVED",
            "2025-03-15T00:00:00Z",
            9001,
        )],
    );
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(move |request: &Request| {
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            let query = body["variables"]["query"].as_str().unwrap();
            let node = if query.contains("author:alice") {
                &authored
            } else {
                &reviewed
            };
            ResponseTemplate::new(200).set_body_json(graphql_search_response(
                std::slice::from_ref(node),
                false,
                None,
            ))
        })
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/testorg/authored/pulls/42/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .expect(1)
        .mount(&ctx.mock_server)
        .await;
    let source = super::super::github_source();
    let plan = source.plan(&ingestion).await.unwrap();
    super::super::super::ongoing_tracking::drive(
        &ingestion,
        &source,
        source.initial_cursor(&ingestion, &plan),
    )
    .await;
    assert_eq!(stored_count(&ctx, &ingestion).await, 2);
    let requests = ctx.mock_server.received_requests().await.unwrap();
    let searches: Vec<_> = requests
        .iter()
        .filter(|request| request.method == "POST")
        .map(|request| {
            serde_json::from_slice::<Value>(&request.body).unwrap()["variables"]["query"]
                .as_str()
                .unwrap()
                .to_owned()
        })
        .collect();
    assert_eq!(searches.len(), 4);
    for org in ["testorg", "secondorg"] {
        for (qualifier, lower) in [("author", "2025-03-01"), ("reviewed-by", "1970-01-01")] {
            let expected = format!(
                "org:{org} {qualifier}:alice created:{lower}T00:00:00Z..2025-03-31T00:00:00Z"
            );
            assert!(
                searches.iter().any(|query| query.contains(&expected)),
                "missing search: {expected}"
            );
        }
    }
    let stored = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(
            &Platform::Github.to_string(),
            &[
                "testorg/old-parent/review/9001".into(),
                "testorg/authored/pull/42".into(),
            ],
        )
        .await
        .unwrap();
    assert_eq!(
        stored.len(),
        2,
        "recent reviews on old PRs must still be discovered"
    );
    ctx.teardown().await;
}
