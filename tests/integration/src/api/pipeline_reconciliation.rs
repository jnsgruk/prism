use ps_core::models::{HandlerMethod, HandlerName, SourceName};
use ps_core::repo::Repos;
use ps_core::repo::activity::PipelineRunParams;
use ps_server::features::dispatch::HandlersServiceImpl;
use uuid::Uuid;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{body_partial_json, body_string_contains, method, path},
};

use crate::common::server::ApiTestContext;

async fn acknowledged_pipeline(repos: &Repos, caller: Uuid) -> Uuid {
    let id = Uuid::now_v7();
    let snapshot = serde_json::json!({"scope":{"kind":"all"}});
    repos
        .activity
        .reserve_pipeline(id, &snapshot, caller, "admin")
        .await
        .unwrap();
    repos
        .activity
        .create_pipeline(id, Some("inv_root"))
        .await
        .unwrap();
    repos
        .activity
        .acknowledge_pipeline_dispatch(id, "inv_root")
        .await
        .unwrap();
    id
}

async fn invocation_status(server: &MockServer, id: &str, rows: serde_json::Value) {
    let query = format!("SELECT id, status FROM sys_invocation WHERE id = '{id}'");
    Mock::given(method("POST"))
        .and(path("/query"))
        .and(body_partial_json(serde_json::json!({"query":query})))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"rows":rows})))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn terminal_acknowledged_root_fails_after_owned_descendants_are_terminal() {
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let id = acknowledged_pipeline(&repos, caller).await;
    let source = SourceName::from("GitHub");
    let handler = HandlerName::from("GithubIngestionHandler");
    let method_name = HandlerMethod::from("run_scoped");
    let owned = Uuid::now_v7();
    let unrelated = Uuid::now_v7();
    repos
        .activity
        .create_pipeline_run(PipelineRunParams {
            run_id: owned,
            source_name: &source,
            handler_name: &handler,
            method: &method_name,
            pipeline_id: id,
            invocation_id: "inv_child",
        })
        .await
        .unwrap();
    repos
        .activity
        .create_run(unrelated, &source, &handler, &method_name)
        .await
        .unwrap();
    let admin = MockServer::start().await;
    invocation_status(
        &admin,
        "inv_root",
        serde_json::json!([{"id":"inv_root","status":"completed"}]),
    )
    .await;
    invocation_status(
        &admin,
        "inv_child",
        serde_json::json!([{"id":"inv_child","status":"completed"}]),
    )
    .await;
    empty_descendant_graph(&admin).await;
    let service = HandlersServiceImpl::new(repos.clone(), admin.uri(), admin.uri());
    service.recover_pipeline_dispatch().await.unwrap();
    let pipeline = repos.activity.get_pipeline(id).await.unwrap().unwrap();
    assert_eq!(pipeline.status, "failed");
    assert!(!pipeline.cancellation_requested);
    assert_eq!(
        pipeline.error.as_deref(),
        Some("Workflow stopped before finalizing")
    );
    assert_eq!(
        repos.activity.get_run(owned).await.unwrap().unwrap().status,
        ps_core::models::IngestionStatus::Failed
    );
    assert_eq!(
        repos
            .activity
            .get_run(unrelated)
            .await
            .unwrap()
            .unwrap()
            .status,
        ps_core::models::IngestionStatus::Running
    );
    repos
        .activity
        .reserve_pipeline(Uuid::now_v7(), &serde_json::json!({}), caller, "admin")
        .await
        .unwrap();
    admin.verify().await;
    ctx.teardown().await;
}

#[tokio::test]
async fn unknown_root_status_holds_acknowledged_admission() {
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let id = acknowledged_pipeline(&repos, caller).await;
    let admin = MockServer::start().await;
    invocation_status(&admin, "inv_root", serde_json::json!([])).await;
    let service = HandlersServiceImpl::new(repos.clone(), admin.uri(), admin.uri());
    service.recover_pipeline_dispatch().await.unwrap();
    assert_eq!(
        repos
            .activity
            .get_pipeline(id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "running"
    );
    assert!(matches!(
        repos
            .activity
            .reserve_pipeline(Uuid::now_v7(), &serde_json::json!({}), caller, "admin")
            .await,
        Err(ps_core::Error::Conflict(_))
    ));
    admin.verify().await;
    ctx.teardown().await;
}

#[tokio::test]
async fn terminal_root_with_live_child_cancels_only_that_child_and_holds_admission() {
    use ps_core::repo::activity::PipelineInvocationParams;
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let id = acknowledged_pipeline(&repos, caller).await;
    repos
        .activity
        .register_pipeline_invocation(PipelineInvocationParams {
            pipeline_id: id,
            invocation_id: "inv_child",
            parent_invocation_id: Some("inv_root"),
            kind: "source",
            run_id: None,
        })
        .await
        .unwrap();
    let admin = MockServer::start().await;
    invocation_status(
        &admin,
        "inv_root",
        serde_json::json!([{"id":"inv_root","status":"completed"}]),
    )
    .await;
    invocation_status(
        &admin,
        "inv_child",
        serde_json::json!([{"id":"inv_child","status":"running"}]),
    )
    .await;
    Mock::given(method("DELETE"))
        .and(path("/invocations/inv_child"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&admin)
        .await;
    empty_descendant_graph(&admin).await;
    let service = HandlersServiceImpl::new(repos.clone(), admin.uri(), admin.uri());
    service.recover_pipeline_dispatch().await.unwrap();
    let pipeline = repos.activity.get_pipeline(id).await.unwrap().unwrap();
    assert_eq!(pipeline.status, "cancelling");
    assert!(!pipeline.cancellation_requested);
    assert!(repos.activity.pipeline_cancel_requested(id).await.unwrap());
    assert!(matches!(
        repos
            .activity
            .reserve_pipeline(Uuid::now_v7(), &serde_json::json!({}), caller, "admin")
            .await,
        Err(ps_core::Error::Conflict(_))
    ));
    admin.verify().await;
    ctx.teardown().await;
}

async fn empty_descendant_graph(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/query"))
        .and(body_string_contains("invoked_by_id IN"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"rows":[]})))
        .expect(1)
        .mount(server)
        .await;
}

#[tokio::test]
async fn legacy_terminal_root_traverses_completed_parent_to_find_exact_unregistered_grandchild() {
    for child_status in ["completed", "running"] {
        let ctx = ApiTestContext::new().await;
        let repos = Repos::new(ctx.server.pool.clone());
        let id = Uuid::now_v7();
        repos
            .activity
            .create_pipeline(id, Some("inv_root"))
            .await
            .unwrap();
        repos.activity.request_pipeline_cancel(id).await.unwrap();
        let unrelated_run = Uuid::now_v7();
        repos
            .activity
            .create_run(
                unrelated_run,
                &SourceName::from("Scheduled GitHub"),
                &HandlerName::from("GithubIngestionHandler"),
                &HandlerMethod::from("run_ingestion"),
            )
            .await
            .unwrap();
        repos.activity.link_runs_to_pipeline(id).await.unwrap();
        let admin = MockServer::start().await;
        invocation_status(
            &admin,
            "inv_root",
            serde_json::json!([{"id":"inv_root", "status":"completed"}]),
        )
        .await;
        for (parent, rows) in [
            (
                "inv_root",
                serde_json::json!([{"id":"inv_middle","invoked_by_id":"inv_root","status":"completed"}]),
            ),
            (
                "inv_middle",
                serde_json::json!([{"id":"inv_grandchild","invoked_by_id":"inv_middle","status":child_status}]),
            ),
            ("inv_grandchild", serde_json::json!([])),
        ] {
            let query = format!(
                "SELECT id, invoked_by_id, status FROM sys_invocation WHERE invoked_by_id IN ('{parent}') LIMIT 10000"
            );
            Mock::given(method("POST"))
                .and(path("/query"))
                .and(body_partial_json(serde_json::json!({"query":query})))
                .respond_with(
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({"rows":rows})),
                )
                .expect(1)
                .mount(&admin)
                .await;
        }
        if child_status == "running" {
            Mock::given(method("DELETE"))
                .and(path("/invocations/inv_grandchild"))
                .respond_with(ResponseTemplate::new(200))
                .expect(1)
                .mount(&admin)
                .await;
        }
        let service = HandlersServiceImpl::new(repos.clone(), admin.uri(), admin.uri());
        service.recover_pipeline_dispatch().await.unwrap();
        let pipeline = repos.activity.get_pipeline(id).await.unwrap().unwrap();
        assert_eq!(
            pipeline.status,
            if child_status == "completed" {
                "cancelled"
            } else {
                "cancelling"
            }
        );
        assert_eq!(
            repos
                .activity
                .get_run(unrelated_run)
                .await
                .unwrap()
                .unwrap()
                .status,
            ps_core::models::IngestionStatus::Running
        );
        admin.verify().await;
        ctx.teardown().await;
    }
}

#[tokio::test]
async fn internal_stop_with_unknown_root_never_becomes_user_cancellation() {
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let id = acknowledged_pipeline(&repos, caller).await;
    repos.activity.request_pipeline_stop(id).await.unwrap();
    let admin = MockServer::start().await;
    invocation_status(&admin, "inv_root", serde_json::json!([])).await;
    let service = HandlersServiceImpl::new(repos.clone(), admin.uri(), admin.uri());
    service.recover_pipeline_dispatch().await.unwrap();
    let pipeline = repos.activity.get_pipeline(id).await.unwrap().unwrap();
    assert_eq!(pipeline.status, "cancelling");
    assert!(!pipeline.cancellation_requested);
    admin.verify().await;
    // The single introspection request is the only HTTP request, with no /cancel.
    assert_eq!(admin.received_requests().await.unwrap().len(), 1);
    ctx.teardown().await;
}

#[tokio::test]
async fn malformed_invocation_receipt_stays_uncertain_and_keeps_original_intent() {
    let ctx = ApiTestContext::new().await;
    let repos = Repos::new(ctx.server.pool.clone());
    let (caller, _) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let id = Uuid::now_v7();
    let snapshot = serde_json::json!({"scope":{"kind":"all"}});
    repos
        .activity
        .reserve_pipeline(id, &snapshot, caller, "admin")
        .await
        .unwrap();
    let ingress = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(202).set_body_json(serde_json::json!({"invocationId":""})),
        )
        .expect(1)
        .mount(&ingress)
        .await;
    let service = HandlersServiceImpl::new(repos.clone(), ingress.uri(), ingress.uri());
    service.recover_pipeline_dispatch().await.unwrap();
    let pipeline = repos.activity.get_pipeline(id).await.unwrap().unwrap();
    assert_eq!(pipeline.status, "pending");
    assert!(!pipeline.dispatch_acknowledged);
    assert!(pipeline.current_invocation_id.is_none());
    assert_eq!(pipeline.request_snapshot, snapshot);
    ingress.verify().await;
    ctx.teardown().await;
}
