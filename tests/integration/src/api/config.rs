use crate::define_api_test;
use ps_proto::prism::v1::config_service_client::ConfigServiceClient;
use ps_proto::prism::v1::{
    CreateSourceRequest, DeleteSourceRequest, GetSourceRequest, ListSourcesRequest,
    SetSecretRequest, TestConnectionRequest, UpdateSourceRequest,
};
use tonic::Request;
use tonic::metadata::MetadataValue;

/// Helper: attach bearer token to a request.
fn auth<T>(req: &mut Request<T>, token: &str) {
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
}

define_api_test!(create_source_and_list, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ConfigServiceClient::new(server.channel.clone());

    // Create a GitHub source
    let mut req = Request::new(CreateSourceRequest {
        source_type: "github".into(),
        name: "My GitHub".into(),
        settings: None,
        schedule_cron: None,
    });
    auth(&mut req, &token);

    let create_resp = client
        .create_source(req)
        .await
        .expect("create_source")
        .into_inner();

    let source = create_resp.source.expect("source should be present");
    assert_eq!(source.source_type, "github");
    assert_eq!(source.name, "My GitHub");
    assert!(source.enabled); // default enabled

    // List sources
    let mut req = Request::new(ListSourcesRequest {});
    auth(&mut req, &token);

    let list_resp = client
        .list_sources(req)
        .await
        .expect("list_sources")
        .into_inner();

    assert_eq!(list_resp.sources.len(), 1);
    assert_eq!(list_resp.sources[0].name, "My GitHub");
});

define_api_test!(get_source_returns_details, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ConfigServiceClient::new(server.channel.clone());

    // Create
    let mut req = Request::new(CreateSourceRequest {
        source_type: "jira".into(),
        name: "My Jira".into(),
        settings: None,
        schedule_cron: Some("0 */6 * * *".into()),
    });
    auth(&mut req, &token);

    let created = client
        .create_source(req)
        .await
        .expect("create_source")
        .into_inner()
        .source
        .expect("source");

    // Get by ID
    let mut req = Request::new(GetSourceRequest {
        source_id: created.id.clone(),
    });
    auth(&mut req, &token);

    let fetched = client
        .get_source(req)
        .await
        .expect("get_source")
        .into_inner()
        .source
        .expect("source");

    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.source_type, "jira");
    assert_eq!(fetched.name, "My Jira");
    assert_eq!(fetched.schedule_cron.as_deref(), Some("0 */6 * * *"));
    assert!(fetched.created_at.is_some());
    assert!(fetched.updated_at.is_some());
});

define_api_test!(update_source_toggles_enabled, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ConfigServiceClient::new(server.channel.clone());

    // Create
    let mut req = Request::new(CreateSourceRequest {
        source_type: "github".into(),
        name: "Toggle Test".into(),
        settings: None,
        schedule_cron: None,
    });
    auth(&mut req, &token);

    let created = client
        .create_source(req)
        .await
        .expect("create_source")
        .into_inner()
        .source
        .expect("source");

    assert!(created.enabled);

    // Disable
    let mut req = Request::new(UpdateSourceRequest {
        source_id: created.id.clone(),
        enabled: Some(false),
        settings: None,
        schedule_cron: None,
    });
    auth(&mut req, &token);

    let updated = client
        .update_source(req)
        .await
        .expect("update_source")
        .into_inner()
        .source
        .expect("source");

    assert!(!updated.enabled);

    // Re-enable
    let mut req = Request::new(UpdateSourceRequest {
        source_id: created.id.clone(),
        enabled: Some(true),
        settings: None,
        schedule_cron: None,
    });
    auth(&mut req, &token);

    let updated = client
        .update_source(req)
        .await
        .expect("update_source")
        .into_inner()
        .source
        .expect("source");

    assert!(updated.enabled);
});

define_api_test!(delete_source, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ConfigServiceClient::new(server.channel.clone());

    // Create
    let mut req = Request::new(CreateSourceRequest {
        source_type: "github".into(),
        name: "To Delete".into(),
        settings: None,
        schedule_cron: None,
    });
    auth(&mut req, &token);

    let created = client
        .create_source(req)
        .await
        .expect("create_source")
        .into_inner()
        .source
        .expect("source");

    // Delete
    let mut req = Request::new(DeleteSourceRequest {
        source_id: created.id.clone(),
    });
    auth(&mut req, &token);

    client.delete_source(req).await.expect("delete_source");

    // Get should return not found
    let mut req = Request::new(GetSourceRequest {
        source_id: created.id.clone(),
    });
    auth(&mut req, &token);

    let err = client
        .get_source(req)
        .await
        .expect_err("get after delete should fail");

    assert_eq!(err.code(), tonic::Code::NotFound);
});

define_api_test!(set_secret_and_test_connection, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ConfigServiceClient::new(server.channel.clone());

    // Create a GitHub source
    let mut req = Request::new(CreateSourceRequest {
        source_type: "github".into(),
        name: "GitHub With Secret".into(),
        settings: None,
        schedule_cron: None,
    });
    auth(&mut req, &token);

    let created = client
        .create_source(req)
        .await
        .expect("create_source")
        .into_inner()
        .source
        .expect("source");

    // Set the required api_token secret
    let mut req = Request::new(SetSecretRequest {
        source_id: created.id.clone(),
        secret_key: "api_token".into(),
        secret_value: "ghp_test_token_value".into(),
    });
    auth(&mut req, &token);

    client.set_secret(req).await.expect("set_secret");

    // Verify the secret_status shows api_token is set (via get_source)
    let mut req = Request::new(GetSourceRequest {
        source_id: created.id.clone(),
    });
    auth(&mut req, &token);

    let fetched = client
        .get_source(req)
        .await
        .expect("get_source")
        .into_inner()
        .source
        .expect("source");

    assert_eq!(fetched.secret_status.get("api_token"), Some(&true));

    // Test connection should succeed
    let mut req = Request::new(TestConnectionRequest {
        source_id: created.id.clone(),
    });
    auth(&mut req, &token);

    let test_resp = client
        .test_connection(req)
        .await
        .expect("test_connection")
        .into_inner();

    assert!(test_resp.success);
    assert!(test_resp.error_message.is_empty());
});

define_api_test!(test_connection_fails_without_secrets, |server| async move {
    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = ConfigServiceClient::new(server.channel.clone());

    // Create a GitHub source without setting any secrets
    let mut req = Request::new(CreateSourceRequest {
        source_type: "github".into(),
        name: "GitHub No Secret".into(),
        settings: None,
        schedule_cron: None,
    });
    auth(&mut req, &token);

    let created = client
        .create_source(req)
        .await
        .expect("create_source")
        .into_inner()
        .source
        .expect("source");

    // Test connection should fail because api_token is missing
    let mut req = Request::new(TestConnectionRequest {
        source_id: created.id.clone(),
    });
    auth(&mut req, &token);

    let test_resp = client
        .test_connection(req)
        .await
        .expect("test_connection")
        .into_inner();

    assert!(!test_resp.success);
    assert!(test_resp.error_message.contains("api_token"));
});
