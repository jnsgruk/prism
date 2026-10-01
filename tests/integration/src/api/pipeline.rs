use crate::common::server::ApiTestContext;
use ps_proto::canonical::prism::v1::handlers_service_client::HandlersServiceClient;
use ps_proto::canonical::prism::v1::{
    CancelPipelineRequest, GetPipelineStatusRequest, TriggerPipelineRequest,
};
use tonic::Request;
use tonic::metadata::MetadataValue;

fn auth<T>(req: &mut Request<T>, token: &str) {
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
}

#[tokio::test]
async fn get_pipeline_status_empty() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = HandlersServiceClient::new(server.channel.clone());

    let mut req = Request::new(GetPipelineStatusRequest::default());
    auth(&mut req, &token);

    let resp = client
        .get_pipeline_status(req)
        .await
        .expect("get_pipeline_status")
        .into_inner();

    assert!(resp.current.is_none());
    assert!(resp.recent.is_empty());

    ctx.teardown().await;
}

#[tokio::test]
async fn get_pipeline_status_with_records() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());

    // Seed a completed pipeline
    let id = uuid::Uuid::now_v7();
    repos.activity.create_pipeline(id, None).await.unwrap();
    let stages = serde_json::json!({
        "ingestion": {"status": "completed", "handlers": [{"name": "Github", "status": "completed", "items": 42}]},
        "metrics": {"status": "completed", "handlers": [{"name": "Metrics", "status": "completed"}]}
    });
    repos
        .activity
        .complete_pipeline(id, "completed", &stages, None)
        .await
        .unwrap();

    let mut client = HandlersServiceClient::new(server.channel.clone());
    let mut req = Request::new(GetPipelineStatusRequest::default());
    auth(&mut req, &token);

    let resp = client
        .get_pipeline_status(req)
        .await
        .expect("get_pipeline_status")
        .into_inner();

    // No running pipeline → current is None, completed one is in recent
    assert!(resp.current.is_none());
    assert_eq!(resp.recent.len(), 1);
    assert_eq!(resp.recent[0].id, id.to_string());
    assert_eq!(resp.recent[0].status, "completed");
    assert!(!resp.recent[0].stages_json.is_empty());

    ctx.teardown().await;
}

#[tokio::test]
async fn get_pipeline_status_running_is_current() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());

    // Seed a running pipeline
    let id = uuid::Uuid::now_v7();
    repos
        .activity
        .create_pipeline(id, Some("inv_123"))
        .await
        .unwrap();

    let mut client = HandlersServiceClient::new(server.channel.clone());
    let mut req = Request::new(GetPipelineStatusRequest::default());
    auth(&mut req, &token);

    let resp = client
        .get_pipeline_status(req)
        .await
        .expect("get_pipeline_status")
        .into_inner();

    assert!(resp.current.is_some());
    assert_eq!(resp.current.unwrap().id, id.to_string());
    assert!(resp.recent.is_empty());

    ctx.teardown().await;
}

#[tokio::test]
async fn trigger_pipeline_rejects_when_active() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());

    // Seed a running pipeline
    let id = uuid::Uuid::now_v7();
    repos.activity.create_pipeline(id, None).await.unwrap();

    let mut client = HandlersServiceClient::new(server.channel.clone());
    let mut req = Request::new(TriggerPipelineRequest::default());
    auth(&mut req, &token);

    let err = client.trigger_pipeline(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::AlreadyExists);

    ctx.teardown().await;
}

#[tokio::test]
async fn cancel_pipeline_not_found() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;

    let mut client = HandlersServiceClient::new(server.channel.clone());
    let mut req = Request::new(CancelPipelineRequest {
        pipeline_id: uuid::Uuid::now_v7().to_string(),
    });
    auth(&mut req, &token);

    let err = client.cancel_pipeline(req).await.unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);

    ctx.teardown().await;
}

#[tokio::test]
async fn concurrent_identical_submissions_return_same_id() {
    let ctx = ApiTestContext::new().await;
    let (_, token) = crate::common::fixtures::create_admin_user(&ctx.server.pool).await;
    let mut first_client = HandlersServiceClient::new(ctx.server.channel.clone());
    let mut second_client = first_client.clone();
    let id = uuid::Uuid::now_v7().to_string();
    let make_request = || {
        let mut request = Request::new(TriggerPipelineRequest {
            submission_id: Some(id.clone()),
            ..Default::default()
        });
        auth(&mut request, &token);
        request
    };
    let (first, second) = tokio::join!(
        first_client.trigger_pipeline(make_request()),
        second_client.trigger_pipeline(make_request()),
    );
    assert_eq!(first.unwrap().into_inner().pipeline_id, id);
    assert_eq!(second.unwrap().into_inner().pipeline_id, id);
    let repos = ps_core::repo::Repos::new(ctx.server.pool.clone());
    assert_eq!(
        repos
            .activity
            .list_recent_pipelines(10)
            .await
            .unwrap()
            .len(),
        1
    );
    let mut wrong_person = Request::new(GetPipelineStatusRequest {
        pipeline_id: Some(id),
        person_id: Some(uuid::Uuid::now_v7().to_string()),
    });
    auth(&mut wrong_person, &token);
    assert_eq!(
        first_client
            .get_pipeline_status(wrong_person)
            .await
            .unwrap_err()
            .code(),
        tonic::Code::NotFound
    );
    ctx.teardown().await;
}
