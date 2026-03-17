use crate::define_api_test;
use ps_proto::prism::v1::config_service_client::ConfigServiceClient;
use ps_proto::prism::v1::handlers_service_client::HandlersServiceClient;
use ps_proto::prism::v1::{
    CreateSourceRequest, GetStatusRequest, ListRunsRequest, TriggerBackfillRequest,
    TriggerRunRequest,
};
use tonic::Request;
use tonic::metadata::MetadataValue;

fn auth<T>(req: &mut Request<T>, token: &str) {
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
}

define_api_test!(get_status_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = HandlersServiceClient::new(server.channel.clone());

    let mut req = Request::new(GetStatusRequest {});
    auth(&mut req, &token);

    let resp = client
        .get_status(req)
        .await
        .expect("get_status")
        .into_inner();
    assert!(resp.sources.is_empty());
});

define_api_test!(get_status_shows_enabled_sources, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;

    // Create an enabled source via ConfigService
    let mut config_client = ConfigServiceClient::new(server.channel.clone());
    let mut req = Request::new(CreateSourceRequest {
        source_type: "github".into(),
        name: "test-github".into(),
        settings: None,
        schedule_cron: None,
    });
    auth(&mut req, &token);
    config_client
        .create_source(req)
        .await
        .expect("create_source");

    // Now check status
    let mut client = HandlersServiceClient::new(server.channel.clone());
    let mut req = Request::new(GetStatusRequest {});
    auth(&mut req, &token);

    let resp = client
        .get_status(req)
        .await
        .expect("get_status")
        .into_inner();
    assert_eq!(resp.sources.len(), 1);
    assert_eq!(resp.sources[0].name, "test-github");
    assert_eq!(resp.sources[0].source_type, "github");
});

define_api_test!(list_runs_empty, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = HandlersServiceClient::new(server.channel.clone());

    let mut req = Request::new(ListRunsRequest {
        source_name: None,
        handler_name: None,
        ingestion_only: false,
    });
    auth(&mut req, &token);

    let resp = client.list_runs(req).await.expect("list_runs").into_inner();
    assert!(resp.runs.is_empty());
});

define_api_test!(list_runs_filters_by_source, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;

    // Insert test fixture runs (using raw query — test-only code is not
    // included by `cargo sqlx prepare`, so the macro form would lack a cache entry)
    sqlx::query(
        "INSERT INTO activity.ingestion_runs (id, source_name, started_at, status) VALUES \
         (gen_random_uuid(), 'src-a', now(), 'completed'), \
         (gen_random_uuid(), 'src-a', now(), 'running'), \
         (gen_random_uuid(), 'src-b', now(), 'completed')",
    )
    .execute(&server.pool)
    .await
    .expect("insert runs");

    let mut client = HandlersServiceClient::new(server.channel.clone());

    // Filter by src-a
    let mut req = Request::new(ListRunsRequest {
        source_name: Some("src-a".into()),
        handler_name: None,
        ingestion_only: false,
    });
    auth(&mut req, &token);

    let resp = client.list_runs(req).await.expect("list_runs").into_inner();
    assert_eq!(resp.runs.len(), 2);
    assert!(resp.runs.iter().all(|r| r.source_name == "src-a"));

    // No filter — all 3
    let mut req = Request::new(ListRunsRequest {
        source_name: None,
        handler_name: None,
        ingestion_only: false,
    });
    auth(&mut req, &token);

    let resp = client.list_runs(req).await.expect("list_runs").into_inner();
    assert_eq!(resp.runs.len(), 3);
});

define_api_test!(trigger_run_requires_valid_source, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = HandlersServiceClient::new(server.channel.clone());

    // Empty name
    let mut req = Request::new(TriggerRunRequest {
        source_name: String::new(),
    });
    auth(&mut req, &token);
    let err = client.trigger_run(req).await.expect_err("should fail");
    assert_eq!(err.code(), tonic::Code::InvalidArgument);

    // Non-existent source
    let mut req = Request::new(TriggerRunRequest {
        source_name: "nonexistent".into(),
    });
    auth(&mut req, &token);
    let err = client.trigger_run(req).await.expect_err("should fail");
    assert_eq!(err.code(), tonic::Code::NotFound);
});

define_api_test!(trigger_backfill_validates_inputs, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = HandlersServiceClient::new(server.channel.clone());

    // Empty source name
    let mut req = Request::new(TriggerBackfillRequest {
        source_name: String::new(),
        since_date: "2024-01-01".into(),
    });
    auth(&mut req, &token);
    let err = client.trigger_backfill(req).await.expect_err("should fail");
    assert_eq!(err.code(), tonic::Code::InvalidArgument);

    // Empty since_date
    let mut req = Request::new(TriggerBackfillRequest {
        source_name: "test".into(),
        since_date: String::new(),
    });
    auth(&mut req, &token);
    let err = client.trigger_backfill(req).await.expect_err("should fail");
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
});

define_api_test!(unauthenticated_requests_rejected, |server| async move {
    let mut client = HandlersServiceClient::new(server.channel.clone());

    // No auth token
    let req = Request::new(GetStatusRequest {});
    let err = client.get_status(req).await.expect_err("should fail");
    assert_eq!(err.code(), tonic::Code::Unauthenticated);
});
