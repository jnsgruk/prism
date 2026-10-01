use ps_core::models::{HandlerMethod, HandlerName, IngestionStatus, SourceName};
use ps_core::repo::Repos;
use ps_server::features::dispatch::HandlersServiceImpl;
use uuid::Uuid;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, method, path, query_param},
};

use crate::common::server::ApiTestContext;

async fn legacy_pipeline(repos: &Repos) -> Uuid {
    let id = Uuid::now_v7();
    repos
        .activity
        .create_pipeline(id, Some("inv_root"))
        .await
        .unwrap();
    repos.activity.request_pipeline_cancel(id).await.unwrap();
    id
}

async fn root_response(server: &MockServer, response: ResponseTemplate) {
    let query = "SELECT id, status FROM sys_invocation WHERE id = 'inv_root'";
    mount_query(server, query, response).await;
}

async fn mount_query(server: &MockServer, query: &str, response: ResponseTemplate) {
    Mock::given(method("POST"))
        .and(path("/query"))
        .and(body_partial_json(serde_json::json!({"query":query})))
        .respond_with(response)
        .expect(1)
        .mount(server)
        .await;
}

fn rows_response(rows: serde_json::Value) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(serde_json::json!({"rows": rows}))
}

async fn root_status(server: &MockServer, status: &str) {
    root_response(
        server,
        rows_response(serde_json::json!([{"id":"inv_root", "status":status}])),
    )
    .await;
}

async fn descendants(server: &MockServer, parent: &str, response: ResponseTemplate) {
    let query = format!(
        "SELECT id, invoked_by_id, status FROM sys_invocation WHERE invoked_by_id IN ('{parent}') LIMIT 10000"
    );
    mount_query(server, &query, response).await;
}

async fn sleeping_chunk_graph(server: &MockServer, chunk_status: &str) {
    for (parent, rows) in [
        (
            "inv_root",
            serde_json::json!([{"id":"inv_middle", "invoked_by_id":"inv_root", "status":"completed"}]),
        ),
        (
            "inv_middle",
            serde_json::json!([{"id":"inv_chunk", "invoked_by_id":"inv_middle", "status":chunk_status}]),
        ),
        ("inv_chunk", serde_json::json!([])),
    ] {
        descendants(server, parent, rows_response(rows)).await;
    }
}

async fn expect_kill(server: &MockServer, id: &str, status: u16) {
    Mock::given(method("DELETE"))
        .and(path(format!("/invocations/{id}")))
        .and(query_param("mode", "kill"))
        .respond_with(ResponseTemplate::new(status))
        .expect(1)
        .mount(server)
        .await;
}

async fn assert_held(repos: &Repos, id: Uuid) {
    let pipeline = repos.activity.get_pipeline(id).await.unwrap().unwrap();
    assert_eq!(pipeline.status, "cancelling");
    assert!(pipeline.cancellation_requested);
    assert!(pipeline.completed_at.is_none());
    assert_eq!(pipeline.current_invocation_id.as_deref(), Some("inv_root"));
    assert!(matches!(
        repos
            .activity
            .reserve_pipeline(
                Uuid::now_v7(),
                &serde_json::json!({}),
                Uuid::now_v7(),
                "admin"
            )
            .await,
        Err(ps_core::Error::Conflict(_))
    ));
}

#[tokio::test]
async fn legacy_cancellation_stops_root_before_draining_sleeping_descendants() {
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let id = legacy_pipeline(&repos).await;
    let partial_run = Uuid::now_v7();
    let unrelated_run = Uuid::now_v7();
    for (run, source) in [(partial_run, "GitHub"), (unrelated_run, "Scheduled Jira")] {
        repos
            .activity
            .create_run(
                run,
                &SourceName::from(source),
                &HandlerName::from("GithubIngestionHandler"),
                &HandlerMethod::from("backfill"),
            )
            .await
            .unwrap();
    }
    repos
        .activity
        .update_run_progress_detail(partial_run, 50, &serde_json::json!({"phase":"collecting"}))
        .await
        .unwrap();
    // Legacy timestamp linking is not proof of ownership for either record.
    repos.activity.link_runs_to_pipeline(id).await.unwrap();

    let admin = MockServer::start().await;
    let service = HandlersServiceImpl::new(repos.clone(), admin.uri(), admin.uri());
    root_status(&admin, "suspended").await;
    expect_kill(&admin, "inv_root", 200).await;
    service.recover_pipeline_dispatch().await.unwrap();
    admin.verify().await;
    assert_eq!(admin.received_requests().await.unwrap().len(), 2);
    assert_held(&repos, id).await;

    // The old root can race with the kill and reach its own finalizer. It must
    // not release admission before the server confirms descendant termination.
    repos
        .activity
        .complete_pipeline(id, "completed", &serde_json::json!({}), None)
        .await
        .unwrap();
    assert_held(&repos, id).await;

    admin.reset().await;
    repos.activity.request_pipeline_cancel(id).await.unwrap();
    root_status(&admin, "completed").await;
    sleeping_chunk_graph(&admin, "suspended").await;
    expect_kill(&admin, "inv_chunk", 200).await;
    service.recover_pipeline_dispatch().await.unwrap();
    admin.verify().await;
    assert_eq!(admin.received_requests().await.unwrap().len(), 5);
    assert_held(&repos, id).await;

    admin.reset().await;
    repos.activity.request_pipeline_cancel(id).await.unwrap();
    root_status(&admin, "completed").await;
    sleeping_chunk_graph(&admin, "completed").await;
    service.recover_pipeline_dispatch().await.unwrap();
    admin.verify().await;
    let requests = admin.received_requests().await.unwrap();
    assert_eq!(requests.len(), 4);
    assert!(requests.iter().all(|request| request.method == "POST"));
    let pipeline = repos.activity.get_pipeline(id).await.unwrap().unwrap();
    assert_eq!(pipeline.status, "cancelled");
    assert!(pipeline.cancellation_requested);
    assert!(pipeline.error.is_none());
    assert!(pipeline.completed_at.is_some());
    assert!(pipeline.current_invocation_id.is_none());
    let partial = repos.activity.get_run(partial_run).await.unwrap().unwrap();
    assert_eq!(partial.items_collected, Some(50));
    assert_eq!(partial.status, IngestionStatus::Running);
    assert_eq!(
        repos
            .activity
            .get_run(unrelated_run)
            .await
            .unwrap()
            .unwrap()
            .status,
        IngestionStatus::Running
    );
    repos
        .activity
        .reserve_pipeline(
            Uuid::now_v7(),
            &serde_json::json!({}),
            Uuid::now_v7(),
            "admin",
        )
        .await
        .unwrap();
    ctx.teardown().await;
}

#[tokio::test]
async fn uncertain_legacy_root_holds_admission_without_killing_or_signalling() {
    for response in [
        rows_response(serde_json::json!([])),
        rows_response(serde_json::json!([{"id":"inv_root", "status":"failed"}])),
        ResponseTemplate::new(200).set_body_json(serde_json::json!({"invalid":[]})),
        ResponseTemplate::new(503),
    ] {
        let ctx = ApiTestContext::new().await;
        let repos = Repos::new(ctx.server.pool.clone());
        let id = legacy_pipeline(&repos).await;
        let admin = MockServer::start().await;
        let service = HandlersServiceImpl::new(repos.clone(), admin.uri(), admin.uri());
        root_response(&admin, response).await;
        service.recover_pipeline_dispatch().await.unwrap();
        admin.verify().await;
        assert_eq!(admin.received_requests().await.unwrap().len(), 1);
        assert_held(&repos, id).await;

        admin.reset().await;
        repos.activity.request_pipeline_cancel(id).await.unwrap();
        root_status(&admin, "completed").await;
        descendants(&admin, "inv_root", rows_response(serde_json::json!([]))).await;
        service.recover_pipeline_dispatch().await.unwrap();
        admin.verify().await;
        assert_eq!(
            repos
                .activity
                .get_pipeline(id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "cancelled"
        );
        ctx.teardown().await;
    }
}

#[tokio::test]
async fn uncertain_legacy_descendant_graph_holds_admission_and_retries() {
    for response in [
        rows_response(
            serde_json::json!([{"id":"inv_chunk", "invoked_by_id":"inv_unrelated", "status":"suspended"}]),
        ),
        rows_response(
            serde_json::json!([{"id":"inv_chunk", "invoked_by_id":"inv_root", "status":"failed"}]),
        ),
        ResponseTemplate::new(200).set_body_string("invalid json"),
        ResponseTemplate::new(503),
    ] {
        let ctx = ApiTestContext::new().await;
        let repos = Repos::new(ctx.server.pool.clone());
        let id = legacy_pipeline(&repos).await;
        let admin = MockServer::start().await;
        let service = HandlersServiceImpl::new(repos.clone(), admin.uri(), admin.uri());
        root_status(&admin, "completed").await;
        descendants(&admin, "inv_root", response).await;
        service.recover_pipeline_dispatch().await.unwrap();
        admin.verify().await;
        assert_eq!(admin.received_requests().await.unwrap().len(), 2);
        assert_held(&repos, id).await;

        admin.reset().await;
        repos.activity.request_pipeline_cancel(id).await.unwrap();
        root_status(&admin, "completed").await;
        descendants(&admin, "inv_root", rows_response(serde_json::json!([]))).await;
        service.recover_pipeline_dispatch().await.unwrap();
        admin.verify().await;
        assert_eq!(
            repos
                .activity
                .get_pipeline(id)
                .await
                .unwrap()
                .unwrap()
                .status,
            "cancelled"
        );
        ctx.teardown().await;
    }
}

#[tokio::test]
async fn rejected_legacy_root_kill_holds_admission_and_retries_exact_root() {
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let id = legacy_pipeline(&repos).await;
    let admin = MockServer::start().await;
    let service = HandlersServiceImpl::new(repos.clone(), admin.uri(), admin.uri());
    for kill_status in [503, 200] {
        repos.activity.request_pipeline_cancel(id).await.unwrap();
        root_status(&admin, "suspended").await;
        expect_kill(&admin, "inv_root", kill_status).await;
        service.recover_pipeline_dispatch().await.unwrap();
        admin.verify().await;
        assert_eq!(admin.received_requests().await.unwrap().len(), 2);
        assert_held(&repos, id).await;
        admin.reset().await;
    }
    ctx.teardown().await;
}

#[tokio::test]
async fn cancelled_legacy_pipeline_without_root_identity_holds_admission() {
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let id = Uuid::now_v7();
    repos.activity.create_pipeline(id, None).await.unwrap();
    repos.activity.request_pipeline_cancel(id).await.unwrap();
    let admin = MockServer::start().await;
    let service = HandlersServiceImpl::new(repos.clone(), admin.uri(), admin.uri());
    service.recover_pipeline_dispatch().await.unwrap();
    assert!(admin.received_requests().await.unwrap().is_empty());
    let pipeline = repos.activity.get_pipeline(id).await.unwrap().unwrap();
    assert_eq!(pipeline.status, "cancelling");
    assert!(pipeline.cancellation_requested);
    assert!(pipeline.completed_at.is_none());
    ctx.teardown().await;
}
