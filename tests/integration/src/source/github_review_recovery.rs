//! Global GitHub coverage must survive failures after review pages commit.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use ps_core::models::{IngestionStatus, Platform, SecretKey};
use ps_core::repo::org::{CreatePersonParams, IdentityInput};
use serde_json::Value;
use uuid::Uuid;
use wiremock::matchers::{method, path};
use wiremock::{Mock, Request, ResponseTemplate};

use crate::common::restate::{RestateTestContext, TEST_SECRET_KEY};
use crate::common::wiremock_helpers::{
    SourceTestContext, graphql_pr_node, graphql_review_node, graphql_search_response,
};

const SOURCE: &str = "github";
const ORIGINAL_WATERMARK: &str = "2025-03-01T00:00:00Z";
const UPDATED: &str = "2025-03-15T12:00:00Z";
const REVIEWER: &str = "known-reviewer";

#[derive(Clone, Copy)]
enum FailureStage {
    Review,
    Repository,
}

struct Provider {
    failure: FailureStage,
    recovered: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
}

impl Provider {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&request.body).unwrap();
        let variables = &body["variables"];
        if let Some(query) = variables["query"].as_str() {
            if query.contains("repo:testorg/later") {
                if matches!(self.failure, FailureStage::Repository) {
                    self.pending.fetch_add(1, Ordering::SeqCst);
                    return self.delayed_outcome(graphql_search_response(&[], false, None));
                }
                return response(graphql_search_response(&[], false, None));
            }
            // Model GitHub's updated filter: advancing past this PR makes it
            // disappear from fresh runs, rather than returning it regardless.
            let expected = format!("repo:testorg/first type:pr updated:>{ORIGINAL_WATERMARK}");
            let nodes = if query == expected {
                vec![parent()]
            } else {
                vec![]
            };
            return response(graphql_search_response(&nodes, false, None));
        }
        match variables["cursor"].as_str().unwrap() {
            "inline-end" => response(review_page(11..111, Some("page-two"))),
            "page-two" => {
                let page = review_page(111..112, None);
                if matches!(self.failure, FailureStage::Review) {
                    self.pending.fetch_add(1, Ordering::SeqCst);
                    self.delayed_outcome(page)
                } else {
                    response(page)
                }
            }
            cursor => panic!("unexpected review cursor: {cursor}"),
        }
    }

    fn delayed_outcome(&self, success: Value) -> ResponseTemplate {
        let result = if self.recovered.load(Ordering::SeqCst) {
            response(success)
        } else {
            ResponseTemplate::new(403).set_body_string("temporarily inaccessible history")
        };
        result.set_delay(Duration::from_secs(2))
    }
}

fn response(body: Value) -> ResponseTemplate {
    ResponseTemplate::new(200)
        .insert_header("x-ratelimit-remaining", "4900")
        .insert_header("x-ratelimit-limit", "5000")
        .insert_header("x-ratelimit-reset", "4102444800")
        .set_body_json(body)
}

fn reviews(ids: std::ops::Range<u64>) -> Vec<Value> {
    ids.map(|id| graphql_review_node(REVIEWER, "APPROVED", UPDATED, id))
        .collect()
}

fn parent() -> Value {
    let mut parent = graphql_pr_node(
        "testorg",
        "first",
        42,
        "unknown-author",
        "Review pagination recovery",
        "OPEN",
        ORIGINAL_WATERMARK,
        UPDATED,
        0,
        0,
        &reviews(1..11),
    );
    parent["reviews"]["totalCount"] = 111.into();
    parent["reviews"]["pageInfo"] =
        serde_json::json!({"hasNextPage":true,"endCursor":"inline-end"});
    parent
}

fn review_page(ids: std::ops::Range<u64>, cursor: Option<&str>) -> Value {
    serde_json::json!({
        "data":{"repository":{"pullRequest":{"reviews":{
            "totalCount":111,
            "pageInfo":{"hasNextPage":cursor.is_some(),"endCursor":cursor},
            "nodes":reviews(ids),
        }}}},
        "extensions":{"rateLimit":{"remaining":4900,"limit":5000,"resetAt":"2099-01-01T00:00:00Z"}},
    })
}

async fn fixture(ctx: &SourceTestContext, failure: FailureStage) {
    let ingestion = ctx
        .build_ingestion_ctx(
            SOURCE,
            Platform::Github,
            serde_json::json!({"base_url":ctx.mock_server.uri(),"orgs":["testorg"]}),
            None,
            None,
            None,
        )
        .await;
    let encrypted = ps_core::crypto::encrypt(&TEST_SECRET_KEY, b"fake-github-test-token").unwrap();
    ctx.repos
        .config
        .upsert_secret(
            Uuid::now_v7(),
            ingestion.source_config.id.into_inner(),
            SecretKey::ApiToken.as_str(),
            &encrypted,
        )
        .await
        .unwrap();
    ctx.repos
        .org
        .create_person(CreatePersonParams {
            name: "Known reviewer".into(),
            email: None,
            level: None,
            team_id: None,
            identities: vec![IdentityInput {
                platform: Platform::Github,
                username: REVIEWER.into(),
                platform_user_id: None,
            }],
        })
        .await
        .unwrap();
    ctx.repos
        .activity
        .upsert_watermark(SOURCE, ORIGINAL_WATERMARK, 73)
        .await
        .unwrap();

    let repositories = match failure {
        FailureStage::Review => vec!["first"],
        FailureStage::Repository => vec!["first", "later"],
    };
    let repos: Vec<_> = repositories
        .iter()
        .map(|repo| {
            serde_json::json!({"name":repo,"full_name":format!("testorg/{repo}"),
                "owner":{"login":"testorg"},"archived":false,"default_branch":"main"})
        })
        .collect();
    Mock::given(method("GET"))
        .and(path("/orgs/testorg/repos"))
        .respond_with(response(serde_json::json!(repos)))
        .expect(2)
        .mount(&ctx.mock_server)
        .await;
    Mock::given(method("GET"))
        .and(path("/repos/testorg/first/pulls/42/files"))
        .respond_with(response(serde_json::json!([])))
        .mount(&ctx.mock_server)
        .await;
}

async fn wait_pending(pending: &AtomicUsize, expected: usize) {
    for _ in 0..100 {
        if pending.load(Ordering::SeqCst) >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("provider did not reach pending target {expected}");
}

async fn coverage(ctx: &SourceTestContext) -> Option<Value> {
    let mut snapshot = sqlx::query_scalar!(
        "SELECT jsonb_agg(to_jsonb(w) ORDER BY source_name) FROM activity.ingestion_watermarks w"
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
    // Invocation/error bookkeeping may change while the source's completed
    // coverage remains fixed. Compare every coverage field, not run ownership.
    if let Some(Value::Array(rows)) = &mut snapshot {
        for row in rows {
            let row = row.as_object_mut().unwrap();
            for field in ["current_invocation_id", "last_attempt", "last_error"] {
                row.remove(field);
            }
        }
    }
    snapshot
}

async fn saved_reviews(ctx: &SourceTestContext) -> Vec<(Uuid, String)> {
    let keys = (1..112)
        .map(|id| format!("testorg/first/review/{id}"))
        .collect::<Vec<_>>();
    ctx.repos
        .activity
        .get_contribution_ids_by_platform_ids(&Platform::Github.to_string(), &keys)
        .await
        .unwrap()
}

async fn assert_recovery(failure: FailureStage) {
    let ctx = SourceTestContext::new().await;
    fixture(&ctx, failure).await;
    let original = coverage(&ctx).await;
    assert!(original.is_some(), "seeded coverage must be observable");
    let recovered = Arc::new(AtomicBool::new(false));
    let pending = Arc::new(AtomicUsize::new(0));
    let provider = Provider {
        failure,
        recovered: recovered.clone(),
        pending: pending.clone(),
    };
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(move |request: &Request| provider.respond(request))
        .mount(&ctx.mock_server)
        .await;

    let runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let failed = runtime.send_github_coordinator(SOURCE).await;
    wait_pending(&pending, 1).await;
    let partial = saved_reviews(&ctx).await;
    let expected = match failure {
        FailureStage::Review => 110,
        FailureStage::Repository => 111,
    };
    assert_eq!(partial.len(), expected, "earlier review pages committed");
    assert_eq!(
        coverage(&ctx).await,
        original,
        "pending work is not source coverage"
    );
    let result = runtime.attach(&failed).await;
    assert!(
        result.status().is_success(),
        "global partial runs return warnings"
    );
    let runs = ctx
        .repos
        .activity
        .list_runs(Some(SOURCE), Some("GithubIngestionHandler"), false)
        .await
        .unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].status, IngestionStatus::CompletedWithWarnings);
    assert_eq!(
        coverage(&ctx).await,
        original,
        "failed target preserves all coverage fields"
    );

    recovered.store(true, Ordering::SeqCst);
    let successful = runtime.send_github_coordinator(SOURCE).await;
    wait_pending(&pending, 2).await;
    assert_eq!(
        coverage(&ctx).await,
        original,
        "fresh run also waits for complete coverage"
    );
    let result = runtime.attach(&successful).await;
    assert!(result.status().is_success());
    let saved = saved_reviews(&ctx).await;
    assert_eq!(saved.len(), 111, "unchanged PR recovers every review");
    assert!(
        partial.iter().all(|pair| saved.contains(pair)),
        "repeat ingestion preserves IDs"
    );
    assert_eq!(
        ctx.repos
            .activity
            .get_watermark(SOURCE)
            .await
            .unwrap()
            .as_deref(),
        Some(UPDATED),
        "complete ingestion publishes its final watermark",
    );
    let runs = ctx
        .repos
        .activity
        .list_runs(Some(SOURCE), Some("GithubIngestionHandler"), false)
        .await
        .unwrap();
    assert!(
        runs.iter()
            .any(|run| run.status == IngestionStatus::Completed)
    );
    let requests = ctx.mock_server.received_requests().await.unwrap();
    let queries: Vec<_> = requests
        .iter()
        .filter(|request| request.url.path() == "/graphql")
        .map(|request| serde_json::from_slice::<Value>(&request.body).unwrap())
        .filter_map(|request| request["variables"]["query"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(
        queries
            .iter()
            .filter(|query| query.contains("repo:testorg/first"))
            .count(),
        2
    );
    assert!(
        queries
            .iter()
            .all(|query| query.contains(&format!("updated:>{ORIGINAL_WATERMARK}")))
    );

    drop(runtime);
    ctx.teardown().await;
}

#[tokio::test]
async fn global_review_page_failure_preserves_coverage_and_fresh_run_recovers() {
    assert_recovery(FailureStage::Review).await;
}

#[tokio::test]
async fn global_later_repository_failure_preserves_completed_reviews_for_fresh_run() {
    assert_recovery(FailureStage::Repository).await;
}
