use ps_core::{
    ingestion::{IngestionPlan, RepoTarget, Source},
    models::Platform,
};
use serde_json::Value;
use wiremock::{
    Mock, Request, ResponseTemplate,
    matchers::{method, path},
};

use super::ongoing_tracking::{drive, person};
use crate::common::wiremock_helpers::{
    SourceTestContext, graphql_pr_node, graphql_search_response,
};

#[tokio::test]
async fn malformed_global_search_nodes_preserve_valid_rows_without_completing_coverage() {
    for team_repo in [true, false] {
        let ctx = SourceTestContext::new().await;
        let ingestion = ctx
            .build_ingestion_ctx(
                "github",
                Platform::Github,
                serde_json::json!({"base_url":ctx.mock_server.uri(),"orgs":["testorg"]}),
                Some("fake-token".into()),
                None,
                None,
            )
            .await;
        person(&ctx, Platform::Github, "manual", None).await;
        let mut valid = graphql_pr_node(
            "testorg",
            "allowed",
            42,
            "manual",
            "Valid remainder",
            "OPEN",
            "2025-03-10T00:00:00Z",
            "2025-03-15T00:00:00Z",
            0,
            0,
            &[],
        );
        valid["repository"]["isArchived"] = false.into();
        let mut nodes = vec![
            valid.clone(),
            Value::Null,
            serde_json::json!({"number":"private-sentinel"}),
        ];
        for field in ["number", "createdAt", "updatedAt", "repository"] {
            let mut incomplete = valid.clone();
            incomplete[field] = Value::Null;
            nodes.push(incomplete);
        }
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(move |request: &Request| {
                let body: Value = serde_json::from_slice(&request.body).unwrap();
                let query = body["variables"]["query"].as_str().unwrap();
                let results = if team_repo && !query.contains("repo:") {
                    &[][..]
                } else {
                    &nodes
                };
                ResponseTemplate::new(200)
                    .set_body_json(graphql_search_response(results, false, None))
            })
            .mount(&ctx.mock_server)
            .await;
        Mock::given(method("GET"))
            .and(path("/repos/testorg/allowed/pulls/42/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .expect(1)
            .mount(&ctx.mock_server)
            .await;
        let source = ps_workers::features::ingestion::github::source::GitHubSource;
        let plan = IngestionPlan {
            source_name: "github".into(),
            discovery_cutoff: None,
            watermark: Some("2025-03-01T00:00:00Z".into()),
            items: vec![],
            repos: if team_repo {
                vec![RepoTarget {
                    owner: "testorg".into(),
                    repo: "allowed".into(),
                }]
            } else {
                vec![]
            },
        };
        let cursor = drive(
            &ingestion,
            &source,
            source.initial_cursor(&ingestion, &plan),
        )
        .await;
        let decoded: Value = serde_json::from_str(&cursor).unwrap();
        assert_eq!(decoded["failed_items"].as_array().unwrap().len(), 6);
        assert_eq!(decoded["max_updated_at"], "2025-03-15T00:00:00Z");
        assert!(decoded["completed_max_updated_at"].is_null());
        assert!(!cursor.contains("private-sentinel"));
        let stored = ctx
            .repos
            .activity
            .get_contribution_ids_by_platform_ids(
                &Platform::Github.to_string(),
                &["testorg/allowed/pull/42".into()],
            )
            .await
            .unwrap();
        assert_eq!(
            stored.len(),
            1,
            "valid activity must survive a malformed neighbor"
        );
        ctx.teardown().await;
    }
}
