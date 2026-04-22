use crate::common::server::ApiTestContext;
use ps_proto::canonical::prism::v1::admin_service_client::AdminServiceClient;
use ps_proto::canonical::prism::v1::backup_service_client::BackupServiceClient;
use ps_proto::canonical::prism::v1::{
    CreateApiTokenRequest, CreateBackupRequest, ListApiTokensRequest, ResetDataRequest,
    RevokeApiTokenRequest,
};
use tonic::Request;
use tonic::metadata::MetadataValue;

fn auth<T>(req: &mut Request<T>, token: &str) {
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
}

// ---------------------------------------------------------------------------
// CreateApiToken + ListApiTokens
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_and_list_api_tokens() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = AdminServiceClient::new(server.channel.clone());

    // Create a token
    let mut req = Request::new(CreateApiTokenRequest {
        name: "ci-token".into(),
    });
    auth(&mut req, &token);

    let resp = client
        .create_api_token(req)
        .await
        .expect("create_api_token")
        .into_inner();

    assert!(!resp.token.is_empty());
    assert_eq!(resp.name, "ci-token");
    assert!(!resp.token_id.is_empty());

    // List tokens — should include the one just created
    let mut req = Request::new(ListApiTokensRequest {});
    auth(&mut req, &token);

    let list = client
        .list_api_tokens(req)
        .await
        .expect("list_api_tokens")
        .into_inner();

    assert_eq!(list.tokens.len(), 1);
    assert_eq!(list.tokens[0].name, "ci-token");

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// RevokeApiToken
// ---------------------------------------------------------------------------

#[tokio::test]
async fn revoke_api_token() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = AdminServiceClient::new(server.channel.clone());

    // Create a token
    let mut req = Request::new(CreateApiTokenRequest {
        name: "to-revoke".into(),
    });
    auth(&mut req, &token);
    let created = client
        .create_api_token(req)
        .await
        .expect("create_api_token")
        .into_inner();

    // Revoke it
    let mut req = Request::new(RevokeApiTokenRequest {
        token_id: created.token_id,
    });
    auth(&mut req, &token);
    client
        .revoke_api_token(req)
        .await
        .expect("revoke_api_token");

    // List — should be empty now
    let mut req = Request::new(ListApiTokensRequest {});
    auth(&mut req, &token);
    let list = client
        .list_api_tokens(req)
        .await
        .expect("list_api_tokens")
        .into_inner();

    assert!(list.tokens.is_empty());

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// RevokeApiToken — not found
// ---------------------------------------------------------------------------

#[tokio::test]
async fn revoke_api_token_not_found() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = AdminServiceClient::new(server.channel.clone());

    let mut req = Request::new(RevokeApiTokenRequest {
        token_id: uuid::Uuid::now_v7().to_string(),
    });
    auth(&mut req, &token);

    let err = client
        .revoke_api_token(req)
        .await
        .expect_err("should be not found");

    assert_eq!(err.code(), tonic::Code::NotFound);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// CreateApiToken — requires auth
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_api_token_requires_auth() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let mut client = AdminServiceClient::new(server.channel.clone());

    let err = client
        .create_api_token(CreateApiTokenRequest {
            name: "no-auth".into(),
        })
        .await
        .expect_err("should require auth");

    assert_eq!(err.code(), tonic::Code::Unauthenticated);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// CreateApiToken — empty name rejected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_api_token_empty_name() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = AdminServiceClient::new(server.channel.clone());

    let mut req = Request::new(CreateApiTokenRequest {
        name: String::new(),
    });
    auth(&mut req, &token);

    let err = client
        .create_api_token(req)
        .await
        .expect_err("empty name should fail");

    assert_eq!(err.code(), tonic::Code::InvalidArgument);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// CreateBackup — streaming response
// ---------------------------------------------------------------------------

#[tokio::test]
async fn create_backup_returns_data() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = BackupServiceClient::new(server.channel.clone());

    let mut req = Request::new(CreateBackupRequest {
        exclude_workspaces: false,
        force: false,
    });
    auth(&mut req, &token);

    let stream = client
        .create_backup(req)
        .await
        .expect("create_backup")
        .into_inner();

    use tokio_stream::StreamExt;
    let chunks: Vec<_> = stream
        .collect::<Vec<_>>()
        .await
        .into_iter()
        .collect::<Result<Vec<_>, _>>()
        .expect("all chunks ok");

    assert!(!chunks.is_empty());
    let total_bytes: usize = chunks
        .iter()
        .filter_map(|c| match &c.payload {
            Some(ps_proto::canonical::prism::v1::create_backup_response::Payload::Chunk(data)) => {
                Some(data.len())
            }
            _ => None,
        })
        .sum();
    assert!(total_bytes > 0, "backup should contain data");

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// ResetData — confirm flag required
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reset_data_requires_confirm() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let mut client = AdminServiceClient::new(server.channel.clone());

    let mut req = Request::new(ResetDataRequest { confirm: false });
    auth(&mut req, &token);

    let err = client
        .reset_data(req)
        .await
        .expect_err("should require confirm");

    assert_eq!(err.code(), tonic::Code::InvalidArgument);

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// ResetData — actually resets
// ---------------------------------------------------------------------------

#[tokio::test]
async fn reset_data_clears_contributions() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let repos = ps_core::repo::Repos::new(server.pool.clone());
    let mut client = AdminServiceClient::new(server.channel.clone());

    // Seed some data
    let person_id = uuid::Uuid::now_v7();
    sqlx::query("INSERT INTO org.people (id, name) VALUES ($1, $2)")
        .bind(person_id)
        .bind("ResetPerson")
        .execute(&server.pool)
        .await
        .unwrap();

    let item = ps_core::ingestion::ContributionInput {
        platform: ps_core::models::Platform::Github,
        contribution_type: ps_core::models::ContributionType::PullRequest,
        platform_id: "RESET-1".into(),
        platform_username: "user".into(),
        title: Some("Test PR".into()),
        url: None,
        state: Some(ps_core::models::ContributionState::Merged),
        created_at: time::OffsetDateTime::now_utc(),
        updated_at: None,
        closed_at: None,
        metrics: serde_json::json!({}),
        metadata: serde_json::json!({}),
        content: None,
        state_history: None,
        enrichment_content: None,
    };

    repos
        .activity
        .upsert_contribution(uuid::Uuid::now_v7(), Some(person_id), &item)
        .await
        .unwrap();

    // Reset
    let mut req = Request::new(ResetDataRequest { confirm: true });
    auth(&mut req, &token);

    let resp = client
        .reset_data(req)
        .await
        .expect("reset_data")
        .into_inner();

    assert!(resp.contributions_deleted >= 1);
    assert!(resp.people_deleted >= 1);

    ctx.teardown().await;
}
