//! Failed later Discourse pages must remain eligible for ordinary ingestion.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use ps_core::models::{IngestionStatus, Platform};
use serde_json::Value;
use uuid::Uuid;
use wiremock::matchers::method;
use wiremock::{Mock, Request, ResponseTemplate};

use crate::common::restate::RestateTestContext;
use crate::common::wiremock_helpers::{
    SourceTestContext, discourse_categories_response, discourse_latest_response, discourse_post,
    discourse_topic_detail, discourse_topic_summary,
};

const SOURCE: &str = "discourse-ubuntu";
const ORIGINAL_WATERMARK: &str = "2025-03-01T00:00:00Z";
const NEWEST: &str = "2025-03-15T12:00:00Z";
const LATER: &str = "2025-03-10T12:00:00Z";

#[derive(Clone, Copy)]
enum FailureStage {
    Detail,
    Liker,
}

struct Provider {
    failure: FailureStage,
    recovered: Arc<AtomicBool>,
    pending: Arc<AtomicUsize>,
}

impl Provider {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        match request.url.path() {
            "/categories.json" => {
                response(discourse_categories_response(&[(1, "General", "general")]))
            }
            "/latest.json" => {
                let page = request
                    .url
                    .query_pairs()
                    .find(|(key, _)| key == "page")
                    .map(|(_, value)| value.into_owned())
                    .unwrap_or_else(|| "0".into());
                match page.as_str() {
                    "0" => response(discourse_latest_response(&[topic(101, NEWEST)], true)),
                    "1" => response(discourse_latest_response(&[topic(102, LATER)], false)),
                    page => panic!("unexpected Discourse page: {page}"),
                }
            }
            "/t/101.json" => response(detail(101, NEWEST)),
            "/t/102.json" => {
                let result = detail(102, LATER);
                if matches!(self.failure, FailureStage::Detail) {
                    self.delayed_outcome(result)
                } else {
                    response(result)
                }
            }
            "/post_action_users.json" => {
                let post_id = request
                    .url
                    .query_pairs()
                    .find(|(key, _)| key == "id")
                    .map(|(_, value)| value.into_owned())
                    .unwrap();
                let result = serde_json::json!({"post_action_users":[{"id":1,"username":"bob"}]});
                if post_id == "1020" && matches!(self.failure, FailureStage::Liker) {
                    self.delayed_outcome(result)
                } else {
                    response(result)
                }
            }
            route => panic!("unexpected provider route: {route}"),
        }
    }

    fn delayed_outcome(&self, success: Value) -> ResponseTemplate {
        self.pending.fetch_add(1, Ordering::SeqCst);
        let response = if self.recovered.load(Ordering::SeqCst) {
            response(success)
        } else {
            ResponseTemplate::new(403).set_body_string("temporarily inaccessible history")
        };
        response.set_delay(Duration::from_secs(2))
    }
}

fn response(body: Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(body)
}

fn topic(id: i64, bumped: &str) -> Value {
    discourse_topic_summary(id, "Topic", "topic", Some(1), 1, ORIGINAL_WATERMARK, bumped)
}

fn detail(id: i64, updated: &str) -> Value {
    let mut post = discourse_post(id * 10, id, "alice", 1, updated, "Body");
    post["like_count"] = 1.into();
    discourse_topic_detail(id, "Topic", "topic", &[post])
}

async fn send_coordinator(runtime: &RestateTestContext) -> String {
    let response = runtime
        .client
        .post(format!(
            "{}/DiscourseIngestionHandler/{SOURCE}/run_ingestion/send",
            runtime.ingress
        ))
        .send()
        .await
        .unwrap();
    let status = response.status();
    let body: Value = response.json().await.unwrap();
    assert!(
        status.is_success(),
        "coordinator send failed: {status} {body}"
    );
    body["invocationId"].as_str().unwrap().into()
}

async fn wait_pending(pending: &AtomicUsize, expected: usize) {
    for _ in 0..100 {
        if pending.load(Ordering::SeqCst) >= expected {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("provider did not reach later-page request {expected}");
}

async fn saved(ctx: &SourceTestContext) -> Vec<(Uuid, String)> {
    let keys = ["101", "102", "like-1010-bob", "like-1020-bob"].map(str::to_owned);
    ctx.repos
        .activity
        .get_contribution_ids_by_platform_ids(
            &Platform::Discourse("ubuntu".into()).to_string(),
            &keys,
        )
        .await
        .unwrap()
}

async fn coverage(ctx: &SourceTestContext) -> Option<Value> {
    let mut snapshot = sqlx::query_scalar!(
        "SELECT jsonb_agg(to_jsonb(w) ORDER BY source_name) FROM activity.ingestion_watermarks w"
    )
    .fetch_one(&ctx.pool)
    .await
    .unwrap();
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

async fn assert_recovery(failure: FailureStage) {
    let ctx = SourceTestContext::new().await;
    ctx.build_ingestion_ctx(
        SOURCE,
        Platform::Discourse("ubuntu".into()),
        serde_json::json!({"base_url":ctx.mock_server.uri(),"fetch_likes":true}),
        None,
        None,
        None,
    )
    .await;
    ctx.repos
        .activity
        .upsert_watermark(SOURCE, ORIGINAL_WATERMARK, 73)
        .await
        .unwrap();
    let original = coverage(&ctx).await;
    assert!(original.is_some(), "seeded coverage must be observable");
    let recovered = Arc::new(AtomicBool::new(false));
    let pending = Arc::new(AtomicUsize::new(0));
    let provider = Provider {
        failure,
        recovered: recovered.clone(),
        pending: pending.clone(),
    };
    Mock::given(method("GET"))
        .respond_with(move |request: &Request| provider.respond(request))
        .mount(&ctx.mock_server)
        .await;

    let runtime = RestateTestContext::new(ctx.repos.clone()).await;
    let failed = send_coordinator(&runtime).await;
    wait_pending(&pending, 1).await;
    let partial = saved(&ctx).await;
    let expected = match failure {
        FailureStage::Detail => 2,
        FailureStage::Liker => 3,
    };
    assert_eq!(partial.len(), expected, "earlier subrequests committed");
    assert_eq!(
        coverage(&ctx).await,
        original,
        "pending pages are not coverage"
    );
    // Legacy coordinators handle a failed ingestion internally; its run record
    // provides the authoritative outcome regardless of the RPC response.
    runtime.attach(&failed).await;
    let runs = ctx
        .repos
        .activity
        .list_runs(Some(SOURCE), Some("DiscourseIngestionHandler"), false)
        .await
        .unwrap();
    assert_eq!(runs.len(), 1);
    assert!(matches!(
        runs[0].status,
        IngestionStatus::Failed | IngestionStatus::CompletedWithWarnings
    ));
    assert_eq!(
        coverage(&ctx).await,
        original,
        "failed fetch preserves completed coverage"
    );

    recovered.store(true, Ordering::SeqCst);
    let successful = send_coordinator(&runtime).await;
    wait_pending(&pending, 2).await;
    assert_eq!(
        coverage(&ctx).await,
        original,
        "rerun also waits for every page"
    );
    let response = runtime.attach(&successful).await;
    assert!(response.status().is_success());
    let complete = saved(&ctx).await;
    assert_eq!(
        complete.len(),
        4,
        "ordinary rerun recovers the missing later history"
    );
    assert!(
        partial.iter().all(|row| complete.contains(row)),
        "natural-key upserts preserve IDs"
    );
    assert_eq!(
        ctx.repos
            .activity
            .get_watermark(SOURCE)
            .await
            .unwrap()
            .as_deref(),
        Some(NEWEST)
    );
    let runs = ctx
        .repos
        .activity
        .list_runs(Some(SOURCE), Some("DiscourseIngestionHandler"), false)
        .await
        .unwrap();
    assert!(
        runs.iter()
            .any(|run| run.status == IngestionStatus::Completed)
    );
    let requests = ctx.mock_server.received_requests().await.unwrap();
    for endpoint in ["/t/101.json", "/t/102.json"] {
        assert_eq!(
            requests
                .iter()
                .filter(|request| request.url.path() == endpoint)
                .count(),
            2,
            "ordinary rerun rediscovers both pages"
        );
    }

    drop(runtime);
    ctx.teardown().await;
}

#[tokio::test]
async fn global_discourse_later_detail_failure_preserves_coverage_and_rerun_recovers() {
    assert_recovery(FailureStage::Detail).await;
}

#[tokio::test]
async fn global_discourse_later_liker_failure_preserves_coverage_and_rerun_recovers() {
    assert_recovery(FailureStage::Liker).await;
}
