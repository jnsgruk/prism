//! A resetting request quota must permit progress across durable pauses.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ps_core::ingestion::Source;
use ps_core::models::{HandlerMethod, HandlerName, Platform, SourceName};
use ps_workers::features::ingestion::discourse::source::DiscourseSource;
use ps_workers::features::ingestion::lib::{chunk::ChunkRequest, finalise::extract_watermark};
use serde_json::Value;
use uuid::Uuid;
use wiremock::{Mock, Request, ResponseTemplate, matchers::method};

use crate::common::restate::RestateTestContext;
use crate::common::wiremock_helpers::{
    SourceTestContext, discourse_categories_response, discourse_latest_response, discourse_post,
    discourse_topic_detail, discourse_topic_summary,
};

const SOURCE: &str = "quota forum";
const ORIGINAL_WATERMARK: &str = "2025-02-01T00:00:00Z";
const UPDATED: &str = "2025-03-15T00:00:00Z";

struct Quota {
    window_started: Instant,
    available: usize,
    successes: BTreeMap<String, usize>,
    pauses: usize,
}

impl Quota {
    fn response(
        &mut self,
        request: &Request,
        bodies: &BTreeMap<String, Value>,
    ) -> ResponseTemplate {
        if self.window_started.elapsed() >= Duration::from_millis(200) {
            self.window_started = Instant::now();
            self.available = 3;
        }
        if self.available == 0 {
            self.pauses += 1;
            return ResponseTemplate::new(429).insert_header("retry-after", "1");
        }
        let key = operation(request);
        let body = bodies
            .get(&key)
            .unwrap_or_else(|| panic!("unexpected Discourse operation: {key}"));
        self.available -= 1;
        *self.successes.entry(key).or_default() += 1;
        ResponseTemplate::new(200).set_body_json(body)
    }
}

fn operation(request: &Request) -> String {
    let path = request.url.path();
    if path == "/post_action_users.json" {
        let post = request
            .url
            .query_pairs()
            .find(|(key, _)| key == "id")
            .unwrap()
            .1;
        format!("{path}?id={post}")
    } else {
        path.into()
    }
}

fn provider_bodies() -> BTreeMap<String, Value> {
    let mut bodies = BTreeMap::from([(
        "/categories.json".into(),
        discourse_categories_response(&[(1, "General", "general")]),
    )]);
    let mut topics = Vec::new();
    for id in 101..104 {
        topics.push(discourse_topic_summary(
            id,
            "Quota recovery",
            "quota-recovery",
            Some(1),
            2,
            "2025-03-01T00:00:00Z",
            UPDATED,
        ));
        let mut posts = Vec::new();
        for number in 1..3 {
            let post_id = id * 10 + i64::from(number);
            let mut post = discourse_post(
                post_id,
                id,
                "author",
                number,
                "2025-03-01T00:00:00Z",
                "A post under a sustained request quota",
            );
            post["like_count"] = 1.into();
            posts.push(post);
            bodies.insert(
                format!("/post_action_users.json?id={post_id}"),
                serde_json::json!({"post_action_users":[{"id":1,"username":"liker"}]}),
            );
        }
        bodies.insert(
            format!("/t/{id}.json"),
            discourse_topic_detail(id, "Quota recovery", "quota-recovery", &posts),
        );
    }
    bodies.insert(
        "/latest.json".into(),
        discourse_latest_response(&topics, false),
    );
    bodies
}

async fn assert_original_watermark(ctx: &SourceTestContext) {
    assert_eq!(
        ctx.repos
            .activity
            .get_watermark(SOURCE)
            .await
            .unwrap()
            .as_deref(),
        Some(ORIGINAL_WATERMARK),
        "partial committed work must not publish completed source coverage"
    );
}

#[tokio::test]
async fn resetting_quota_completes_each_operation_once_across_durable_sleep_and_restart() {
    let ctx = SourceTestContext::new().await;
    let platform = Platform::Discourse("ubuntu".into());
    let mut ingestion = ctx
        .build_ingestion_ctx(
            SOURCE,
            platform.clone(),
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
    let run_id = Uuid::now_v7();
    ctx.repos
        .activity
        .create_run(
            run_id,
            &SourceName::new(SOURCE),
            &HandlerName::new("DiscourseIngestionHandler"),
            &HandlerMethod::new("run_ingestion"),
        )
        .await
        .unwrap();
    ingestion.run_id = Some(run_id);

    let bodies = provider_bodies();
    let expected_operations: Vec<_> = bodies.keys().cloned().collect();
    let quota = Arc::new(Mutex::new(Quota {
        window_started: Instant::now(),
        available: 3,
        successes: BTreeMap::new(),
        pauses: 0,
    }));
    let provider_quota = quota.clone();
    Mock::given(method("GET"))
        .respond_with(move |request: &Request| {
            provider_quota.lock().unwrap().response(request, &bodies)
        })
        .mount(&ctx.mock_server)
        .await;

    let source = DiscourseSource;
    let plan = source.plan(&ingestion).await.unwrap();
    let mut request = ChunkRequest {
        source_type: platform.clone(),
        cursor: source.initial_cursor(&ingestion, &plan),
        run_id,
        max_batches: 3,
        items_offset: 0,
        request: None,
    };
    let mut runtime = RestateTestContext::new(ctx.repos.clone()).await;
    runtime.shorten_chunk_inactivity_timeout().await;
    let first_invocation = runtime.send("process_chunk", &request, None).await;
    let first = runtime.result(&first_invocation).await;
    assert!(!first.is_complete);
    assert_eq!(first.items_stored, 2, "first topic and reply commit");
    let partial: Value = serde_json::from_str(&first.cursor).unwrap();
    assert_eq!(partial["max_bumped_at"], UPDATED);
    assert_eq!(
        extract_watermark(&first.cursor, source.watermark_field()),
        None,
        "raw progress cannot masquerade as completed coverage"
    );
    assert_original_watermark(&ctx).await;

    request.cursor = first.cursor;
    request.items_offset = first.items_stored;
    request.max_batches = 50;
    let invocation = runtime.send("process_chunk", &request, None).await;
    let sleep_query = format!(
        "SELECT entry_type FROM sys_journal WHERE id = '{invocation}' AND entry_type = 'Command: Sleep'"
    );
    let mut sleeping = false;
    for _ in 0..100 {
        if !runtime.query(&sleep_query).await.is_empty() {
            sleeping = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(sleeping, "quota must produce a durable sleep");
    assert_original_watermark(&ctx).await;
    runtime.restart_worker().await;

    let result = runtime.result(&invocation).await;
    assert!(result.is_complete, "bounded quota retries must finish");
    assert_eq!(first.items_stored + result.items_stored, 12);
    assert_eq!(
        extract_watermark(&result.cursor, source.watermark_field()).as_deref(),
        Some(UPDATED)
    );
    assert_eq!(
        ctx.repos
            .activity
            .get_watermark(SOURCE)
            .await
            .unwrap()
            .as_deref(),
        Some(UPDATED)
    );
    let keys: Vec<_> = (101..104)
        .flat_map(|id| {
            [
                id.to_string(),
                (id * 10 + 2).to_string(),
                format!("like-{}-liker", id * 10 + 1),
                format!("like-{}-liker", id * 10 + 2),
            ]
        })
        .collect();
    let saved = ctx
        .repos
        .activity
        .get_contribution_ids_by_platform_ids(&platform.to_string(), &keys)
        .await
        .unwrap();
    assert_eq!(
        saved.len(),
        keys.len(),
        "every topic, reply and like persists"
    );
    {
        let quota = quota.lock().unwrap();
        assert!(quota.pauses >= 2, "quota must reset repeatedly");
        assert_eq!(quota.successes.len(), expected_operations.len());
        for operation in expected_operations {
            assert_eq!(
                quota.successes.get(&operation),
                Some(&1),
                "completed operation must not be replayed: {operation}"
            );
        }
    }
    drop(runtime);
    ctx.teardown().await;
}
