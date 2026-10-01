use ps_core::models::{HandlerMethod, HandlerName, SourceName};
use ps_core::repo::activity::{PipelineInvocationParams, PipelineRunParams};
use uuid::Uuid;

use crate::common::server::ApiTestContext;

#[tokio::test]
async fn cancellation_closes_registration_and_only_cancels_owned_runs() {
    let ctx = ApiTestContext::new().await;
    let repos = ps_core::repo::Repos::new(ctx.server.pool.clone());
    let pipeline_id = Uuid::now_v7();
    repos
        .activity
        .create_pipeline(pipeline_id, Some("inv_parent"))
        .await
        .unwrap();
    let owned_id = Uuid::now_v7();
    let unrelated_id = Uuid::now_v7();
    let source = SourceName::from("GitHub");
    let handler = HandlerName::from("GithubIngestionHandler");
    let method = HandlerMethod::from("run_scoped");
    assert!(
        repos
            .activity
            .create_pipeline_run(PipelineRunParams {
                run_id: owned_id,
                source_name: &source,
                handler_name: &handler,
                method: &method,
                pipeline_id,
                invocation_id: "inv_owned",
            })
            .await
            .unwrap()
    );
    repos
        .activity
        .create_run(unrelated_id, &source, &handler, &method)
        .await
        .unwrap();
    assert!(
        repos
            .activity
            .register_pipeline_invocation(PipelineInvocationParams {
                pipeline_id,
                invocation_id: "inv_owned",
                parent_invocation_id: Some("inv_parent"),
                kind: "source",
                run_id: Some(owned_id),
            })
            .await
            .unwrap()
    );
    repos
        .activity
        .request_pipeline_cancel(pipeline_id)
        .await
        .unwrap();
    assert!(
        repos
            .activity
            .pipeline_cancel_requested(pipeline_id)
            .await
            .unwrap()
    );
    assert!(
        !repos
            .activity
            .register_pipeline_invocation(PipelineInvocationParams {
                pipeline_id,
                invocation_id: "inv_late",
                parent_invocation_id: Some("inv_parent"),
                kind: "chunk",
                run_id: None,
            })
            .await
            .unwrap()
    );
    assert!(
        !repos
            .activity
            .create_pipeline_run(PipelineRunParams {
                run_id: Uuid::now_v7(),
                source_name: &source,
                handler_name: &handler,
                method: &method,
                pipeline_id,
                invocation_id: "inv_late_run",
            })
            .await
            .unwrap()
    );
    assert_eq!(
        repos
            .activity
            .list_pipeline_invocation_ids(pipeline_id)
            .await
            .unwrap(),
        vec!["inv_owned"]
    );
    // A completion racing with cancellation must retain the cancelled outcome
    // and finalize its owned rows within the same transaction.
    let effective = repos
        .activity
        .finish_owned_pipeline(ps_core::repo::activity::PipelineFinishParams {
            pipeline_id,
            status: "completed",
            stages: &serde_json::json!({}),
            error: None,
            parent_run_id: Some(owned_id),
        })
        .await
        .unwrap();
    assert_eq!(effective, "cancelled");

    assert_eq!(
        repos
            .activity
            .get_run(owned_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        ps_core::models::IngestionStatus::Cancelled
    );
    assert_eq!(
        repos
            .activity
            .get_run(unrelated_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        ps_core::models::IngestionStatus::Running
    );
    assert_eq!(
        repos
            .activity
            .get_pipeline(pipeline_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "cancelled"
    );
    ctx.teardown().await;
}

#[tokio::test]
async fn atomic_owner_finalization_preserves_completed_child_data_and_finishes_parent() {
    use ps_core::repo::activity::PipelineFinishParams;
    let ctx = ApiTestContext::new().await;
    let repos = ps_core::repo::Repos::new(ctx.server.pool.clone());
    let pipeline_id = Uuid::now_v7();
    repos
        .activity
        .create_pipeline(pipeline_id, Some("inv_root"))
        .await
        .unwrap();
    let root_id = Uuid::now_v7();
    let child_id = Uuid::now_v7();
    let source = SourceName::from("GitHub");
    let handler = HandlerName::from("GithubIngestionHandler");
    let method = HandlerMethod::from("run_scoped");
    for (run_id, invocation_id) in [(root_id, "inv_root"), (child_id, "inv_child")] {
        assert!(
            repos
                .activity
                .create_pipeline_run(PipelineRunParams {
                    run_id,
                    source_name: &source,
                    handler_name: &handler,
                    method: &method,
                    pipeline_id,
                    invocation_id,
                })
                .await
                .unwrap()
        );
    }
    repos
        .activity
        .complete_pipeline_run(child_id, 42)
        .await
        .unwrap();
    let effective = repos
        .activity
        .finish_owned_pipeline(PipelineFinishParams {
            pipeline_id,
            status: "completed",
            stages: &serde_json::json!({}),
            error: None,
            parent_run_id: Some(root_id),
        })
        .await
        .unwrap();
    assert_eq!(effective, "completed");
    assert_eq!(
        repos
            .activity
            .get_run(root_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        ps_core::models::IngestionStatus::Completed
    );
    let child = repos.activity.get_run(child_id).await.unwrap().unwrap();
    assert_eq!(child.status, ps_core::models::IngestionStatus::Completed);
    assert_eq!(child.items_collected, Some(42));
    assert_eq!(
        repos
            .activity
            .get_pipeline(pipeline_id)
            .await
            .unwrap()
            .unwrap()
            .status,
        "completed"
    );
    ctx.teardown().await;
}
