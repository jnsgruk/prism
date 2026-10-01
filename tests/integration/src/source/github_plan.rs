use super::*;

#[tokio::test]
async fn plan_falls_back_to_org_discovery() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    // Mock the REST org repos endpoint for fallback discovery
    let repos_body = serde_json::json!([{
        "name": "repo1",
        "full_name": "testorg/repo1",
        "owner": { "login": "testorg" },
        "archived": false,
    }]);

    Mock::given(method("GET"))
        .and(path("/orgs/testorg/repos"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(&repos_body)
                .append_header("x-ratelimit-remaining", "4999")
                .append_header("x-ratelimit-limit", "5000")
                .append_header("x-ratelimit-reset", "9999999999"),
        )
        .mount(&ctx.mock_server)
        .await;

    let source = github_source();
    let plan = source.plan(&ing_ctx).await.expect("plan");

    assert_eq!(plan.source_name, "github");
    assert_eq!(plan.repos.len(), 1);
    assert_eq!(plan.repos[0].owner, "testorg");
    assert_eq!(plan.repos[0].repo, "repo1");
    // Watermark should be set (default lookback)
    assert!(plan.watermark.is_some());

    ctx.teardown().await;
}

#[tokio::test]
async fn plan_with_existing_watermark() {
    let ctx = SourceTestContext::new().await;

    let settings = github_settings(&ctx.mock_server.uri(), &["testorg"]);
    let ing_ctx = ctx
        .build_ingestion_ctx(
            "github",
            Platform::Github,
            settings,
            Some("test-token".into()),
            None,
            None,
        )
        .await;

    // Set a watermark first
    ctx.repos
        .activity
        .upsert_watermark("github", "2025-03-01T00:00:00Z", 50)
        .await
        .unwrap();

    // Mock REST org repos
    Mock::given(method("GET"))
        .and(path("/orgs/testorg/repos"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!([{
                    "name": "repo1",
                    "full_name": "testorg/repo1",
                    "owner": { "login": "testorg" },
                    "archived": false,
                }]))
                .append_header("x-ratelimit-remaining", "4999")
                .append_header("x-ratelimit-limit", "5000")
                .append_header("x-ratelimit-reset", "9999999999"),
        )
        .mount(&ctx.mock_server)
        .await;

    let source = github_source();
    let plan = source.plan(&ing_ctx).await.expect("plan");
    assert_eq!(plan.watermark.as_deref(), Some("2025-03-01T00:00:00Z"));

    ctx.teardown().await;
}
