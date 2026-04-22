use crate::common::server::ApiTestContext;
use ps_proto::canonical::prism::v1::backup_service_client::BackupServiceClient;
use ps_proto::canonical::prism::v1::{
    CancelBackupRequest, CreateBackupRequest, PreviewBackupRequest, RestoreBackupRequest,
};
use sqlx::PgPool;
use tonic::Request;
use tonic::metadata::MetadataValue;
use uuid::Uuid;

fn auth<T>(req: &mut Request<T>, token: &str) {
    req.metadata_mut().insert(
        "authorization",
        MetadataValue::try_from(format!("Bearer {token}")).expect("valid metadata"),
    );
}

const CHUNK_SIZE: usize = 256 * 1024;

/// Seed representative data across multiple backed-up tables.
///
/// Returns `(person_id, team_id, contribution_id)` for later verification.
async fn seed_data(pool: &PgPool, user_id: Uuid) -> (Uuid, Uuid, Uuid) {
    let person_id = Uuid::now_v7();
    sqlx::query("INSERT INTO org.people (id, name, email) VALUES ($1, $2, $3)")
        .bind(person_id)
        .bind("Alice Test")
        .bind("alice@example.com")
        .execute(pool)
        .await
        .expect("insert person");

    let team_id = Uuid::now_v7();
    sqlx::query("INSERT INTO org.teams (id, name, org_name, lead_id) VALUES ($1, $2, $3, $4)")
        .bind(team_id)
        .bind("Engineering")
        .bind("TestOrg")
        .bind(person_id)
        .execute(pool)
        .await
        .expect("insert team");

    let identity_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO org.platform_identities (id, person_id, platform, platform_username) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(identity_id)
    .bind(person_id)
    .bind("github")
    .bind("alicetest")
    .execute(pool)
    .await
    .expect("insert identity");

    let membership_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO org.team_memberships (id, person_id, team_id, start_date) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(membership_id)
    .bind(person_id)
    .bind(team_id)
    .bind(time::Date::from_calendar_date(2024, time::Month::January, 1).unwrap())
    .execute(pool)
    .await
    .expect("insert membership");

    let repo_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO org.repositories (id, github_org, github_repo, team_id) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(repo_id)
    .bind("testorg")
    .bind("testrepo")
    .bind(team_id)
    .execute(pool)
    .await
    .expect("insert repository");

    // Source config
    let source_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO config.source_configs (id, source_type, name, settings) \
         VALUES ($1, $2, $3, $4)",
    )
    .bind(source_id)
    .bind("github")
    .bind("test-github")
    .bind(serde_json::json!({"org": "testorg"}))
    .execute(pool)
    .await
    .expect("insert source config");

    // Global setting
    sqlx::query("INSERT INTO config.global_settings (key, value) VALUES ($1, $2)")
        .bind("test_setting")
        .bind(serde_json::json!({"enabled": true}))
        .execute(pool)
        .await
        .expect("insert global setting");

    // Watermark
    sqlx::query(
        "INSERT INTO activity.ingestion_watermarks (source_name, watermark_value) \
         VALUES ($1, $2)",
    )
    .bind("test-github")
    .bind("2024-01-01T00:00:00Z")
    .execute(pool)
    .await
    .expect("insert watermark");

    // Contribution
    let contribution_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO activity.contributions \
         (id, person_id, platform, contribution_type, platform_id, title, state, created_at, metrics, metadata) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)",
    )
    .bind(contribution_id)
    .bind(person_id)
    .bind("github")
    .bind("pull_request")
    .bind("PR-123")
    .bind("Fix the thing")
    .bind("merged")
    .bind(time::OffsetDateTime::now_utc())
    .bind(serde_json::json!({"lines_added": 10}))
    .bind(serde_json::json!({"repo": "testorg/testrepo"}))
    .execute(pool)
    .await
    .expect("insert contribution");

    // Conversation
    let conversation_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO reasoning.conversations \
         (id, user_id, title, status, model_name, container_status, query_status) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(conversation_id)
    .bind(user_id)
    .bind("Test conversation")
    .bind("active")
    .bind("test-model")
    .bind("pending")
    .bind("idle")
    .execute(pool)
    .await
    .expect("insert conversation");

    // Conversation message
    let message_id = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO reasoning.conversation_messages \
         (id, conversation_id, role, content) VALUES ($1, $2, $3, $4)",
    )
    .bind(message_id)
    .bind(conversation_id)
    .bind("user")
    .bind("What is the team velocity?")
    .execute(pool)
    .await
    .expect("insert message");

    (person_id, team_id, contribution_id)
}

/// Collect a CreateBackup server-streaming response into raw bytes.
async fn create_backup_bytes(
    client: &mut BackupServiceClient<tonic::transport::Channel>,
    token: &str,
) -> Vec<u8> {
    let mut req = Request::new(CreateBackupRequest {
        exclude_workspaces: true,
        force: false,
    });
    auth(&mut req, token);

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

    chunks
        .into_iter()
        .filter_map(|msg| match msg.payload {
            Some(ps_proto::canonical::prism::v1::create_backup_response::Payload::Chunk(data)) => {
                Some(data)
            }
            _ => None,
        })
        .flatten()
        .collect()
}

/// Count rows in a table (returns i64).
async fn count_rows(pool: &PgPool, table: &str) -> i64 {
    let query = format!("SELECT COUNT(*)::bigint FROM {table}");
    let row: (i64,) = sqlx::query_as(&query)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|e| panic!("count {table}: {e}"));
    row.0
}

// ---------------------------------------------------------------------------
// Backup and restore roundtrip
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn backup_and_restore_roundtrip() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    // 1. Create admin user and seed data
    let (user_id, token) = crate::common::fixtures::create_admin_user(&server.pool).await;
    let (_person_id, _team_id, _contribution_id) = seed_data(&server.pool, user_id).await;

    // Capture pre-backup counts
    let pre_people = count_rows(&server.pool, "org.people").await;
    let pre_teams = count_rows(&server.pool, "org.teams").await;
    let pre_contributions = count_rows(&server.pool, "activity.contributions").await;
    let pre_users = count_rows(&server.pool, "auth.users").await;
    let pre_conversations = count_rows(&server.pool, "reasoning.conversations").await;
    let pre_messages = count_rows(&server.pool, "reasoning.conversation_messages").await;

    assert!(pre_people >= 1);
    assert!(pre_teams >= 1);
    assert!(pre_contributions >= 1);

    // 2. Create backup via gRPC
    let mut backup_client = BackupServiceClient::new(server.channel.clone());
    let backup_bytes = create_backup_bytes(&mut backup_client, &token).await;
    assert!(!backup_bytes.is_empty(), "backup should not be empty");

    // 3. Preview backup via gRPC (with auth — instance is initialised)
    let preview_stream = tokio_stream::iter(
        backup_bytes
            .chunks(CHUNK_SIZE)
            .map(|chunk| PreviewBackupRequest {
                chunk: chunk.to_vec(),
            })
            .collect::<Vec<_>>(),
    );
    let mut preview_req = Request::new(preview_stream);
    auth(&mut preview_req, &token);
    let preview = backup_client
        .preview_backup(preview_req)
        .await
        .expect("preview_backup")
        .into_inner();

    assert_eq!(preview.schema_version, 2);
    assert!(preview.secret_key_valid, "secret key should be valid");
    assert!(preview.checksum_valid, "checksum should be valid");

    // 4. Restore backup via gRPC (with auth — instance is initialised)
    let restore_stream = tokio_stream::iter(
        backup_bytes
            .chunks(CHUNK_SIZE)
            .map(|chunk| RestoreBackupRequest {
                chunk: chunk.to_vec(),
            })
            .collect::<Vec<_>>(),
    );
    let mut restore_req = Request::new(restore_stream);
    auth(&mut restore_req, &token);
    let restore = backup_client
        .restore_backup(restore_req)
        .await
        .expect("restore_backup")
        .into_inner();

    assert!(
        !restore.session_token.is_empty(),
        "restore should return a session token"
    );

    // 5. Verify row counts match pre-backup state
    let post_people = count_rows(&server.pool, "org.people").await;
    let post_teams = count_rows(&server.pool, "org.teams").await;
    let post_contributions = count_rows(&server.pool, "activity.contributions").await;
    let post_users = count_rows(&server.pool, "auth.users").await;
    let post_conversations = count_rows(&server.pool, "reasoning.conversations").await;
    let post_messages = count_rows(&server.pool, "reasoning.conversation_messages").await;

    assert_eq!(post_people, pre_people, "people count mismatch");
    assert_eq!(post_teams, pre_teams, "teams count mismatch");
    assert_eq!(
        post_contributions, pre_contributions,
        "contributions count mismatch"
    );
    assert_eq!(post_users, pre_users, "users count mismatch");
    assert_eq!(
        post_conversations, pre_conversations,
        "conversations count mismatch"
    );
    assert_eq!(post_messages, pre_messages, "messages count mismatch");

    // 6. Verify specific field values survived the roundtrip
    let person_name: (String,) =
        sqlx::query_as("SELECT name FROM org.people WHERE email = 'alice@example.com'")
            .fetch_one(&server.pool)
            .await
            .expect("find alice");
    assert_eq!(person_name.0, "Alice Test");

    let team_name: (String,) = sqlx::query_as("SELECT name FROM org.teams LIMIT 1")
        .fetch_one(&server.pool)
        .await
        .expect("find team");
    assert_eq!(team_name.0, "Engineering");

    let contribution_title: (Option<String>,) =
        sqlx::query_as("SELECT title FROM activity.contributions WHERE platform_id = 'PR-123'")
            .fetch_one(&server.pool)
            .await
            .expect("find contribution");
    assert_eq!(contribution_title.0.as_deref(), Some("Fix the thing"));

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Conditional auth tests
// ---------------------------------------------------------------------------

/// On an uninitialised instance (no users), preview should work without auth.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_without_auth_on_fresh_instance() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    // No users created — instance is uninitialised.
    // Send a minimal (invalid) backup to test the auth gate, not the backup logic.
    let mut backup_client = BackupServiceClient::new(server.channel.clone());
    let preview_stream = tokio_stream::iter(vec![PreviewBackupRequest {
        chunk: b"not a real backup".to_vec(),
    }]);

    // Should not get UNAUTHENTICATED — the request should reach the handler
    // and fail with INVALID_ARGUMENT (bad backup format), not auth rejection.
    let result = backup_client.preview_backup(preview_stream).await;
    match result {
        Err(status) => {
            assert_ne!(
                status.code(),
                tonic::Code::Unauthenticated,
                "fresh instance should not require auth for preview"
            );
        }
        Ok(_) => {
            // Unexpected success with garbage data, but auth passed — that's fine
        }
    }

    ctx.teardown().await;
}

/// On an initialised instance, preview without auth should be rejected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn preview_without_auth_on_live_instance_rejected() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    // Create an admin user — instance is now initialised
    let (_user_id, _token) = crate::common::fixtures::create_admin_user(&server.pool).await;

    let mut backup_client = BackupServiceClient::new(server.channel.clone());
    let preview_stream = tokio_stream::iter(vec![PreviewBackupRequest {
        chunk: b"not a real backup".to_vec(),
    }]);

    // Should get UNAUTHENTICATED — no auth header on a live instance
    let result = backup_client.preview_backup(preview_stream).await;
    assert!(result.is_err(), "should be rejected without auth");
    assert_eq!(
        result.unwrap_err().code(),
        tonic::Code::Unauthenticated,
        "live instance should require auth for preview"
    );

    ctx.teardown().await;
}

/// On an initialised instance, restore without auth should be rejected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn restore_without_auth_on_live_instance_rejected() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    // Create an admin user — instance is now initialised
    let (_user_id, _token) = crate::common::fixtures::create_admin_user(&server.pool).await;

    let mut backup_client = BackupServiceClient::new(server.channel.clone());
    let restore_stream = tokio_stream::iter(vec![RestoreBackupRequest {
        chunk: b"not a real backup".to_vec(),
    }]);

    // Should get UNAUTHENTICATED — no auth header on a live instance
    let result = backup_client.restore_backup(restore_stream).await;
    assert!(result.is_err(), "should be rejected without auth");
    assert_eq!(
        result.unwrap_err().code(),
        tonic::Code::Unauthenticated,
        "live instance should require auth for restore"
    );

    ctx.teardown().await;
}

// ---------------------------------------------------------------------------
// Cancellation test
// ---------------------------------------------------------------------------

/// Cancelling when no backup is running returns cancelled=false.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancel_backup_when_none_active() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;

    let (_user_id, token) = crate::common::fixtures::create_admin_user(&server.pool).await;

    let mut cancel_client = BackupServiceClient::new(server.channel.clone());
    let mut cancel_req = Request::new(CancelBackupRequest {});
    auth(&mut cancel_req, &token);
    let cancel_resp = cancel_client
        .cancel_backup(cancel_req)
        .await
        .expect("cancel_backup RPC should succeed");
    assert!(
        !cancel_resp.into_inner().cancelled,
        "should return false when no backup is active"
    );

    ctx.teardown().await;
}
