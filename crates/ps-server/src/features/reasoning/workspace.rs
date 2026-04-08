use base64::Engine;
use ps_proto::canonical::prism::v1::{
    GetWorkspaceFileRequest, GetWorkspaceFileResponse, ListWorkspaceFilesRequest,
    ListWorkspaceFilesResponse, SyncWorkspaceFilesRequest, SyncWorkspaceFilesResponse,
    WorkspaceFileInfo,
};
use std::path::{Path, PathBuf};
use tonic::{Request, Response, Status};
use tracing::debug;
use uuid::Uuid;

use super::ReasoningServiceImpl;
use crate::common::{db_err, require_auth};

/// Maximum number of files returned by a workspace listing.
const MAX_FILES: usize = 10_000;
/// Maximum directory depth to recurse.
const MAX_DEPTH: usize = 20;

/// Directory names to exclude from workspace listings.
const HIDDEN_DIRS: &[&str] = &[
    ".git",
    ".opencode",
    "__pycache__",
    ".cache",
    "node_modules",
    ".venv",
    ".mypy_cache",
    ".ruff_cache",
];

fn is_hidden(path: &Path) -> bool {
    path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        HIDDEN_DIRS.iter().any(|p| s == *p)
    })
}

fn guess_content_type(filename: &str) -> &'static str {
    match filename.rsplit('.').next() {
        Some("csv") => "text/csv",
        Some("json" | "jsonl") => "application/json",
        Some("md" | "mdx") => "text/markdown",
        Some("txt" | "log" | "nix" | "proto" | "graphql" | "gql" | "dockerfile") => "text/plain",
        Some("html" | "htm") => "text/html",
        Some("css" | "scss") => "text/css",
        Some("js" | "mjs" | "cjs") => "text/javascript",
        Some("ts" | "tsx") => "application/typescript",
        Some("py") => "text/x-python",
        Some("rs") => "text/x-rust",
        Some("go") => "text/x-go",
        Some("rb") => "text/x-ruby",
        Some("java") => "text/x-java-source",
        Some("sh" | "bash" | "zsh") => "text/x-shellscript",
        Some("sql") => "text/x-sql",
        Some("yaml" | "yml") => "text/yaml",
        Some("toml") => "text/x-toml",
        Some("xml" | "svg") => "application/xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("pdf") => "application/pdf",
        Some("zip") => "application/zip",
        Some("gz" | "tgz") => "application/gzip",
        Some("tar") => "application/x-tar",
        _ => "application/octet-stream",
    }
}

// ---------------------------------------------------------------------------
// Filesystem helpers (synchronous — called inside spawn_blocking)
// ---------------------------------------------------------------------------

fn workspace_dir(workspaces_path: &Path, conv_id: &str) -> PathBuf {
    workspaces_path.join(conv_id)
}

fn list_files_recursive(root: &Path) -> Vec<WorkspaceFileInfo> {
    let mut files = Vec::new();
    // (directory_path, depth)
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if depth > MAX_DEPTH || files.len() >= MAX_FILES {
            break;
        }

        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            if files.len() >= MAX_FILES {
                break;
            }

            let path = entry.path();
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };

            if is_hidden(relative) {
                continue;
            }

            let Ok(metadata) = entry.metadata() else {
                continue;
            };

            if metadata.is_dir() {
                stack.push((path.clone(), depth + 1));
                files.push(WorkspaceFileInfo {
                    path: relative.to_string_lossy().to_string(),
                    size_bytes: 0,
                    is_directory: true,
                    modified_unix: modified_time(&metadata),
                    content_type: None,
                });
            } else if metadata.is_file() {
                let rel_str = relative.to_string_lossy().to_string();
                files.push(WorkspaceFileInfo {
                    path: rel_str.clone(),
                    #[allow(clippy::cast_possible_wrap)]
                    size_bytes: metadata.len() as i64,
                    is_directory: false,
                    modified_unix: modified_time(&metadata),
                    content_type: Some(guess_content_type(&rel_str).to_string()),
                });
            }
        }
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    files
}

fn read_file_as_data_url(
    workspaces_path: &Path,
    conv_id: &str,
    path: &str,
) -> Result<(String, String, i64), String> {
    let workspace_root = workspace_dir(workspaces_path, conv_id);
    let file_path = workspace_root.join(path);

    // Canonicalize both to prevent path traversal via symlinks.
    let canonical = file_path
        .canonicalize()
        .map_err(|_| "file not found".to_string())?;
    let canonical_root = workspace_root
        .canonicalize()
        .map_err(|_| "workspace not found".to_string())?;

    if !canonical.starts_with(&canonical_root) {
        return Err("invalid path".to_string());
    }

    let data = std::fs::read(&canonical).map_err(|e| format!("read failed: {e}"))?;
    let content_type = guess_content_type(path).to_string();
    #[allow(clippy::cast_possible_wrap)]
    let size_bytes = data.len() as i64;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
    let download_url = format!("data:{content_type};base64,{b64}");

    Ok((download_url, content_type, size_bytes))
}

#[allow(clippy::cast_possible_wrap)]
fn modified_time(metadata: &std::fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

// ---------------------------------------------------------------------------
// RPC handlers
// ---------------------------------------------------------------------------

pub async fn list_workspace_files(
    svc: &ReasoningServiceImpl,
    request: Request<ListWorkspaceFilesRequest>,
) -> Result<Response<ListWorkspaceFilesResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let conv_id: Uuid = req
        .conversation_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

    let _conv = svc
        .repos
        .reasoning
        .get_conversation(conv_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("conversation not found"))?;

    let Some(ref workspaces_path) = svc.workspaces_path else {
        return Ok(Response::new(ListWorkspaceFilesResponse {
            files: vec![],
            source: "unavailable".to_string(),
        }));
    };

    let dir = workspace_dir(workspaces_path, &conv_id.to_string());
    let files = tokio::task::spawn_blocking(move || list_files_recursive(&dir))
        .await
        .map_err(|_| Status::internal("task failed"))?;

    debug!(conversation_id = %conv_id, count = files.len(), "listed workspace files");

    let source = if files.is_empty() {
        "unavailable"
    } else {
        "live"
    };

    Ok(Response::new(ListWorkspaceFilesResponse {
        files,
        source: source.to_string(),
    }))
}

pub async fn get_workspace_file(
    svc: &ReasoningServiceImpl,
    request: Request<GetWorkspaceFileRequest>,
) -> Result<Response<GetWorkspaceFileResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let conv_id: Uuid = req
        .conversation_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

    if req.path.is_empty() || req.path.contains("..") {
        return Err(Status::invalid_argument("invalid path"));
    }

    let _conv = svc
        .repos
        .reasoning
        .get_conversation(conv_id)
        .await
        .map_err(db_err)?
        .ok_or_else(|| Status::not_found("conversation not found"))?;

    let workspaces_path = svc
        .workspaces_path
        .as_ref()
        .ok_or_else(|| Status::unavailable("workspace storage not configured"))?
        .clone();

    let path = req.path.clone();
    let cid = conv_id.to_string();
    let (download_url, content_type, size_bytes) =
        tokio::task::spawn_blocking(move || read_file_as_data_url(&workspaces_path, &cid, &path))
            .await
            .map_err(|_| Status::internal("task failed"))?
            .map_err(Status::not_found)?;

    Ok(Response::new(GetWorkspaceFileResponse {
        download_url,
        content_type,
        size_bytes,
    }))
}

pub async fn sync_workspace_files(
    _svc: &ReasoningServiceImpl,
    _request: Request<SyncWorkspaceFilesRequest>,
) -> Result<Response<SyncWorkspaceFilesResponse>, Status> {
    // No longer needed with shared PVC — workspace files are always available.
    Ok(Response::new(SyncWorkspaceFilesResponse {
        synced_count: 0,
    }))
}
