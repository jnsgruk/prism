//! Real coordinator/finalizer coverage for person ingestion outcomes.

use ps_core::models::{IngestionStatus, Platform};
use wiremock::matchers::{method, path};
use wiremock::{Mock, Request, ResponseTemplate};

use super::scoped_chunks::{ACCOUNT, issue, scoped_jira};
use crate::common::restate::RestateTestContext;
use crate::common::wiremock_helpers::{SourceTestContext, jira_search_response};

#[derive(Clone, Copy)]
enum Outcome {
    Success,
    Empty,
    Partial,
    Failed,
}

async fn assert_coordinator_outcome(outcome: Outcome) {
    let ctx = SourceTestContext::new().await;
    let mut ingestion = scoped_jira(&ctx).await;
    let projects = match outcome {
        Outcome::Success | Outcome::Empty => vec!["PROJ"],
        Outcome::Partial | Outcome::Failed => vec!["PROJ", "BAD"],
    };
    ingestion.source_config.settings["projects"] = serde_json::json!(projects);
    ctx.repos
        .config
        .update_source_settings(
            ingestion.source_config.id.into_inner(),
            &ingestion.source_config.settings,
        )
        .await
        .unwrap();
    ctx.repos
        .activity
        .upsert_watermark("unrelated Jira", "unrelated-checkpoint", 91)
        .await
        .unwrap();
    let before = sqlx::query_scalar!(
        "SELECT jsonb_agg(to_jsonb(w) ORDER BY source_name) FROM activity.ingestion_watermarks w"
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert!(
        before.is_some(),
        "coverage snapshot must contain both sources"
    );
    Mock::given(method("GET"))
        .and(path("/rest/api/3/search/jql"))
        .respond_with(move |request: &Request| {
            let query = request
                .url
                .query_pairs()
                .find(|(key, _)| key == "jql")
                .unwrap()
                .1;
            let failed = matches!(outcome, Outcome::Failed)
                || matches!(outcome, Outcome::Partial) && query.contains("project = \"BAD\"");
            if failed {
                ResponseTemplate::new(403).set_body_string("private provider failure body")
            } else {
                let items = if matches!(outcome, Outcome::Empty) {
                    vec![]
                } else {
                    vec![issue("PROJ-1", ACCOUNT)]
                };
                ResponseTemplate::new(200).set_body_json(jira_search_response(&items, true, None))
            }
        })
        .expect(projects.len() as u64)
        .mount(&ctx.mock_server)
        .await;

    let runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let request = ingestion.request.as_ref().unwrap();
    let invocation = runtime.send_jira_coordinator(request).await;
    let response = runtime.attach(&invocation).await;
    assert_eq!(
        response.status().is_success(),
        matches!(outcome, Outcome::Success | Outcome::Empty)
    );
    let body = response.text().await.unwrap();
    if matches!(outcome, Outcome::Partial | Outcome::Failed) {
        assert!(body.contains("person history is incomplete"), "{body}");
    }
    let runs = ctx
        .repos
        .activity
        .list_runs(Some("selected Jira"), Some("JiraIngestionHandler"), false)
        .await
        .unwrap();
    let run = runs
        .iter()
        .find(|run| run.id != ingestion.run_id.unwrap())
        .unwrap();
    assert_eq!(run.pipeline_id, Some(request.pipeline_id));
    assert!(run.completed_at.is_some());
    let expected_status = match outcome {
        Outcome::Success | Outcome::Empty => IngestionStatus::Completed,
        Outcome::Partial => IngestionStatus::CompletedWithWarnings,
        Outcome::Failed => IngestionStatus::Failed,
    };
    assert_eq!(run.status, expected_status);
    let expected_items = i32::from(matches!(outcome, Outcome::Success | Outcome::Partial));
    if !matches!(outcome, Outcome::Failed) {
        assert_eq!(run.items_collected, Some(expected_items));
    }
    let metadata = sqlx::query_scalar!(
        "SELECT metadata FROM activity.ingestion_runs WHERE id = $1",
        run.id
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap()
    .unwrap();
    let failed = match outcome {
        Outcome::Success | Outcome::Empty => 0,
        Outcome::Partial => 1,
        Outcome::Failed => 2,
    };
    assert_eq!(metadata["failed_items"].as_array().unwrap().len(), failed);
    assert_eq!(metadata["since_date"], "2025-03-01");
    assert_eq!(metadata["source_id"], request.source.source_id.to_string());
    assert_eq!(
        metadata["run_started_at"],
        serde_json::to_value(request.run_started_at).unwrap()
    );
    let coverage = metadata["coverage"].as_array().unwrap();
    assert_eq!(coverage.len(), 1);
    assert!(coverage[0].as_str().unwrap().contains("current assignee"));
    assert!(
        coverage[0]
            .as_str()
            .unwrap()
            .contains("visible to the API user")
    );
    assert!(
        !metadata
            .to_string()
            .contains("private provider failure body")
    );
    let progress = run.progress.as_ref().unwrap();
    assert_eq!(progress["coverage"], metadata["coverage"]);
    assert_eq!(progress["failed_items"], metadata["failed_items"]);
    let after = sqlx::query_scalar!(
        "SELECT jsonb_agg(to_jsonb(w) ORDER BY source_name) FROM activity.ingestion_watermarks w"
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    assert_eq!(
        after, before,
        "every global coverage field must remain exactly unchanged"
    );
    let saved = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(&Platform::Jira.to_string(), &["PROJ-1".into()])
        .await
        .unwrap();
    assert_eq!(saved.len(), usize::try_from(expected_items).unwrap());
    let owned = ctx
        .repos
        .activity
        .list_pipeline_invocation_ids(request.pipeline_id)
        .await
        .unwrap();
    assert!(owned.contains(&invocation));
    assert!(
        owned.len() >= 3,
        "reserved fixture, coordinator and chunk owned"
    );
    drop(runtime);
    ctx.teardown().await;
}

#[tokio::test]
async fn scoped_success_finalizes_without_mutating_any_global_coverage_field() {
    assert_coordinator_outcome(Outcome::Success).await;
}

#[tokio::test]
async fn scoped_empty_finalizes_with_frozen_coverage_without_mutating_global_fields() {
    assert_coordinator_outcome(Outcome::Empty).await;
}

#[tokio::test]
async fn scoped_partial_failure_keeps_coverage_and_cannot_report_success() {
    assert_coordinator_outcome(Outcome::Partial).await;
}

#[tokio::test]
async fn scoped_all_failed_finalizes_failed_with_coverage_and_no_global_writes() {
    assert_coordinator_outcome(Outcome::Failed).await;
}
