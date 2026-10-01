//! Combined download lifecycle against real PostgreSQL and temporary storage.

use crate::common::{fixtures::create_admin_user, server::ApiTestContext};
use ps_core::repo::{Repos, reasoning::CreateConversationParams, reasoning::CreateMessageParams};
use ps_proto::canonical::prism::v1::{
    DeleteConversationRequest, DownloadWorkspaceFileRequest, GetConversationRequest,
    ResolveWorkspaceFilesRequest, WorkspaceFileAvailability,
    reasoning_service_client::ReasoningServiceClient,
};
use tonic::{Request, transport::Channel};
use uuid::Uuid;

const REPORT: &str = "Activity_Report_2026.pdf";

fn authenticated<T>(body: T, token: &str) -> Request<T> {
    let mut request = Request::new(body);
    request.metadata_mut().insert(
        "authorization",
        format!("Bearer {token}").parse().expect("authorization"),
    );
    request
}

async fn download(
    client: &mut ReasoningServiceClient<Channel>,
    conversation_id: Uuid,
    path: &str,
    token: &str,
) -> (Vec<u8>, String, i64, usize) {
    let mut stream = client
        .download_workspace_file(authenticated(
            DownloadWorkspaceFileRequest {
                conversation_id: conversation_id.to_string(),
                path: path.into(),
            },
            token,
        ))
        .await
        .expect("download starts")
        .into_inner();
    let first = stream
        .message()
        .await
        .expect("first frame")
        .expect("metadata");
    let content_type = first.content_type;
    let size = first.total_size_bytes;
    let mut bytes = first.data;
    let mut chunks = 1;

    while let Some(frame) = stream.message().await.expect("complete stream") {
        bytes.extend(frame.data);
        chunks += 1;
    }

    (bytes, content_type, size, chunks)
}

#[tokio::test]
async fn workspace_download_lifecycle_legacy_reload_expiry_and_deletion() {
    let ctx = ApiTestContext::new().await;
    let server = &ctx.server;
    let (user_id, token) = create_admin_user(&server.pool).await;
    let repos = Repos::new(server.pool.clone());
    let conversation = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            id: None,
            user_id,
            title: Some("Workspace release fixture"),
            model_name: "fixture",
        })
        .await
        .expect("conversation");
    let id = conversation.id;
    let directory = server.workspaces_dir.path().join(id.to_string());
    std::fs::create_dir_all(directory.join("nested")).expect("workspace");
    let pdf = b"%PDF-1.4\nfixture report\n%%EOF\n";
    std::fs::write(directory.join(REPORT), pdf).expect("PDF fixture");
    let nested_path = "nested/résumé 100% #?.csv";
    let csv = b"person,count\nfixture,42\n".repeat(10_000);
    std::fs::write(directory.join(nested_path), &csv).expect("multi-chunk fixture");
    std::fs::write(directory.join("empty.csv"), []).expect("empty fixture");

    // Existing history remains unchanged; the frontend repairs this exact shape.
    let legacy = format!("[/workspace/{REPORT}](/workspace/{REPORT})");
    repos
        .reasoning
        .create_message(&CreateMessageParams {
            conversation_id: id,
            role: "assistant",
            content: &legacy,
            reasoning_trace: None,
            supporting_data: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            attached_files: &[],
            mentions: &serde_json::json!([]),
        })
        .await
        .expect("legacy message");
    let mut client = ReasoningServiceClient::new(server.channel.clone());
    let history = client
        .get_conversation(authenticated(
            GetConversationRequest {
                conversation_id: id.to_string(),
            },
            &token,
        ))
        .await
        .expect("reload history")
        .into_inner();
    assert_eq!(history.messages[0].content, legacy);

    let request = ResolveWorkspaceFilesRequest {
        conversation_id: id.to_string(),
        paths: vec![REPORT.into(), nested_path.into(), "empty.csv".into()],
    };
    let metadata = client
        .resolve_workspace_files(authenticated(request.clone(), &token))
        .await
        .expect("resolve fixtures")
        .into_inner();
    assert_eq!(metadata.files.len(), 3);
    assert!(
        metadata
            .files
            .iter()
            .all(|file| file.availability == i32::from(WorkspaceFileAvailability::Available))
    );
    assert_eq!(metadata.files[0].path, REPORT);
    assert_eq!(metadata.files[0].content_type, "application/pdf");
    assert_eq!(
        metadata.files[0].size_bytes,
        i64::try_from(pdf.len()).unwrap()
    );

    let (bytes, mime, size, _) = download(&mut client, id, REPORT, &token).await;
    assert_eq!(bytes, pdf);
    assert_eq!(mime, "application/pdf");
    assert_eq!(size, i64::try_from(pdf.len()).unwrap());
    let (bytes, _, size, chunks) = download(&mut client, id, nested_path, &token).await;
    assert_eq!(bytes, csv);
    assert_eq!(size, i64::try_from(csv.len()).unwrap());
    assert!(chunks > 1, "fixture must exercise chunk assembly");
    let (bytes, _, size, chunks) = download(&mut client, id, "empty.csv", &token).await;
    assert!(bytes.is_empty());
    assert_eq!(size, 0);
    assert_eq!(chunks, 1, "empty files still have a metadata frame");

    // No Kubernetes pod is created: simulate its expired DB lifecycle state and
    // prove resolution/download depend on persistent storage, not pod presence.
    repos
        .reasoning
        .update_container_status(id, Some("isolated-expired-pod"), "expired", None, None)
        .await
        .expect("expire fixture pod state");
    let (bytes, _, _, _) = download(&mut client, id, REPORT, &token).await;
    assert_eq!(bytes, pdf);

    std::fs::remove_file(directory.join(REPORT)).expect("remove isolated report");
    let metadata = client
        .resolve_workspace_files(authenticated(request.clone(), &token))
        .await
        .expect("recheck after deletion")
        .into_inner();
    assert_eq!(
        metadata.files[0].availability,
        i32::from(WorkspaceFileAvailability::Missing)
    );
    let error = client
        .download_workspace_file(authenticated(
            DownloadWorkspaceFileRequest {
                conversation_id: id.to_string(),
                path: REPORT.into(),
            },
            &token,
        ))
        .await
        .expect_err("deleted file cannot download");
    assert_eq!(error.code(), tonic::Code::NotFound);

    client
        .delete_conversation(authenticated(
            DeleteConversationRequest {
                conversation_id: id.to_string(),
            },
            &token,
        ))
        .await
        .expect("delete isolated conversation");
    assert!(
        directory.join(nested_path).exists(),
        "storage survives until async cleanup"
    );
    let error = client
        .resolve_workspace_files(authenticated(request, &token))
        .await
        .expect_err("deleted conversation cannot resolve orphaned storage");
    assert_eq!(error.code(), tonic::Code::NotFound);
    let error = client
        .download_workspace_file(authenticated(
            DownloadWorkspaceFileRequest {
                conversation_id: id.to_string(),
                path: nested_path.into(),
            },
            &token,
        ))
        .await
        .expect_err("deleted conversation cannot download orphaned storage");
    assert_eq!(error.code(), tonic::Code::NotFound);

    ctx.teardown().await;
}

#[tokio::test]
async fn workspace_download_lifecycle_auth_fails_closed() {
    let ctx = ApiTestContext::new().await;
    let (user_id, _) = create_admin_user(&ctx.server.pool).await;
    let repos = Repos::new(ctx.server.pool.clone());
    let expired_token = ps_core::auth::generate_token();
    repos
        .auth
        .create_session(
            Uuid::now_v7(),
            user_id,
            &ps_core::auth::hash_token(&expired_token),
            "browser",
            Some(time::OffsetDateTime::now_utc() - time::Duration::hours(1)),
            None,
        )
        .await
        .expect("expired fixture session");
    let mut client = ReasoningServiceClient::new(ctx.server.channel.clone());
    let id = Uuid::now_v7().to_string();
    let request = ResolveWorkspaceFilesRequest {
        conversation_id: id.clone(),
        paths: vec![REPORT.into()],
    };
    let error = client
        .resolve_workspace_files(request.clone())
        .await
        .expect_err("missing auth");
    assert_eq!(error.code(), tonic::Code::Unauthenticated);
    let error = client
        .resolve_workspace_files(authenticated(request, &expired_token))
        .await
        .expect_err("invalid session");
    assert_eq!(error.code(), tonic::Code::Unauthenticated);
    let error = client
        .download_workspace_file(DownloadWorkspaceFileRequest {
            conversation_id: id,
            path: REPORT.into(),
        })
        .await
        .expect_err("missing download auth");
    assert_eq!(error.code(), tonic::Code::Unauthenticated);

    ctx.teardown().await;
}

#[tokio::test]
async fn workspace_resolution_shared_read_policy_and_confined_paths() {
    let ctx = ApiTestContext::new().await;
    let (owner, token) = create_admin_user(&ctx.server.pool).await;
    let repos = Repos::new(ctx.server.pool.clone());
    let conv = repos
        .reasoning
        .create_conversation(&CreateConversationParams {
            id: None,
            user_id: owner,
            title: None,
            model_name: "fixture",
        })
        .await
        .expect("conversation");
    let other = Uuid::new_v4();
    repos
        .auth
        .create_user(
            other,
            "reader",
            "Reader",
            "fixture hash",
            ps_core::models::Role::Admin,
        )
        .await
        .expect("reader");
    let reader_token = ps_core::auth::generate_token();
    repos
        .auth
        .create_session(
            Uuid::new_v4(),
            other,
            &ps_core::auth::hash_token(&reader_token),
            "browser",
            Some(time::OffsetDateTime::now_utc() + time::Duration::days(1)),
            None,
        )
        .await
        .expect("reader session");
    let directory = ctx.server.workspaces_dir.path().join(conv.id.to_string());
    std::fs::create_dir_all(directory.join("directory")).expect("directory");
    std::fs::write(directory.join("report.pdf"), b"%PDF").expect("file");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc/passwd", directory.join("escape.txt")).expect("symlink");
    let mut client = ReasoningServiceClient::new(ctx.server.channel.clone());
    let files = client
        .resolve_workspace_files(authenticated(
            ResolveWorkspaceFilesRequest {
                conversation_id: conv.id.to_string(),
                paths: vec![
                    "report.pdf".into(),
                    "directory".into(),
                    "../escape".into(),
                    "/etc/passwd".into(),
                    "escape.txt".into(),
                    "missing".into(),
                ],
            },
            &token,
        ))
        .await
        .expect("resolve")
        .into_inner()
        .files;
    assert_eq!(
        files[0].availability,
        WorkspaceFileAvailability::Available as i32
    );
    let shared = client
        .resolve_workspace_files(authenticated(
            ResolveWorkspaceFilesRequest {
                conversation_id: conv.id.to_string(),
                paths: vec!["report.pdf".into()],
            },
            &reader_token,
        ))
        .await
        .expect("authenticated shared read")
        .into_inner();
    assert_eq!(
        shared.files[0].availability,
        WorkspaceFileAvailability::Available as i32
    );
    let (reader_bytes, _, _, _) = download(&mut client, conv.id, "report.pdf", &reader_token).await;
    assert_eq!(reader_bytes, b"%PDF");
    for paths in [vec!["report.pdf".into(); 129], vec!["x".repeat(4097)]] {
        let error = client
            .resolve_workspace_files(authenticated(
                ResolveWorkspaceFilesRequest {
                    conversation_id: conv.id.to_string(),
                    paths,
                },
                &token,
            ))
            .await
            .expect_err("bounded metadata request");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }
    let deduplicated = client
        .resolve_workspace_files(authenticated(
            ResolveWorkspaceFilesRequest {
                conversation_id: conv.id.to_string(),
                paths: vec!["report.pdf".into(); 128],
            },
            &token,
        ))
        .await
        .expect("deduplicated metadata")
        .into_inner();
    assert_eq!(deduplicated.files.len(), 1);
    let storage = ctx.server.workspaces_dir.path();
    let offline = storage.with_extension("offline");
    std::fs::rename(storage, &offline).expect("simulate storage outage");
    let outage = client
        .resolve_workspace_files(authenticated(
            ResolveWorkspaceFilesRequest {
                conversation_id: conv.id.to_string(),
                paths: vec!["report.pdf".into()],
            },
            &token,
        ))
        .await
        .expect("outage metadata")
        .into_inner();
    std::fs::rename(&offline, storage).expect("restore fixture storage");
    assert_eq!(
        outage.files[0].availability,
        WorkspaceFileAvailability::Unavailable as i32
    );
    for file in &files[1..5] {
        assert_eq!(file.availability, WorkspaceFileAvailability::Invalid as i32);
    }
    assert_eq!(
        files[5].availability,
        WorkspaceFileAvailability::Missing as i32
    );
    for path in ["directory", "../escape", "/etc/passwd", "escape.txt"] {
        let error = client
            .download_workspace_file(authenticated(
                DownloadWorkspaceFileRequest {
                    conversation_id: conv.id.to_string(),
                    path: path.into(),
                },
                &token,
            ))
            .await
            .expect_err("reject unsafe download");
        assert_eq!(error.code(), tonic::Code::InvalidArgument);
    }
    let error = client
        .resolve_workspace_files(authenticated(
            ResolveWorkspaceFilesRequest {
                conversation_id: Uuid::new_v4().to_string(),
                paths: vec!["report.pdf".into()],
            },
            &token,
        ))
        .await
        .expect_err("unknown conversation");
    assert_eq!(error.code(), tonic::Code::NotFound);
    ctx.teardown().await;
}
