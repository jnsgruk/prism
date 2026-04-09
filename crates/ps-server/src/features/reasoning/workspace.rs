use base64::Engine;
use ps_proto::canonical::prism::v1::{
    DownloadWorkspaceFileRequest, DownloadWorkspaceFileResponse, GetWorkspaceFileRequest,
    GetWorkspaceFileResponse, ListWorkspaceFilesRequest, ListWorkspaceFilesResponse,
    UploadWorkspaceFileRequest, UploadWorkspaceFileResponse, WorkspaceFileInfo,
};
use std::path::{Path, PathBuf};
use tokio::io::AsyncReadExt;
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
    // Check well-known extensionless filenames first.
    let basename = filename.rsplit('/').next().unwrap_or(filename);
    match basename.to_ascii_uppercase().as_str() {
        "DOCKERFILE" | "MAKEFILE" | "RAKEFILE" | "GEMFILE" | "PROCFILE" | "LICENSE" | "LICENCE"
        | "COPYING" | "AUTHORS" | "CONTRIBUTORS" | "CHANGELOG" | "README" | "CODEOWNERS"
        | "JUSTFILE" => return "text/plain",
        _ => {}
    }
    // Check if the filename starts with a dot but has no further extension
    // (e.g. .gitignore, .dockerignore, .editorconfig).
    if basename.starts_with('.') && !basename[1..].contains('.') {
        return "text/plain";
    }
    // Files with a TAG suffix (e.g. CACHEDIR.TAG) or no recognised extension.
    match filename.rsplit('.').next() {
        Some("csv") => "text/csv",
        Some("json" | "jsonl") => "application/json",
        Some("md" | "mdx") => "text/markdown",
        Some(
            "txt" | "log" | "lock" | "cfg" | "ini" | "env" | "nix" | "proto" | "graphql" | "gql"
            | "dockerfile" | "tag" | "conf" | "properties" | "gitignore" | "dockerignore"
            | "editorconfig",
        ) => "text/plain",
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

/// Check whether raw bytes look like text (valid UTF-8, no null bytes).
fn looks_like_text(data: &[u8]) -> bool {
    // Check a prefix — no need to scan multi-MB binaries.
    let sample = data.get(..8192).unwrap_or(data);
    !sample.contains(&0) && std::str::from_utf8(sample).is_ok()
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
    let mut content_type = guess_content_type(path).to_string();
    // If the extension-based guess gave up, sniff the actual content.
    if content_type == "application/octet-stream" && looks_like_text(&data) {
        content_type = "text/plain".to_string();
    }
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

/// Maximum upload file size (100 MB).
const MAX_UPLOAD_SIZE: usize = 100 * 1024 * 1024;

/// Stream file content in ~64KB chunks.
const CHUNK_SIZE: usize = 65_536;

pub type DownloadWorkspaceFileStream =
    tokio_stream::wrappers::ReceiverStream<Result<DownloadWorkspaceFileResponse, Status>>;

pub async fn download_workspace_file(
    svc: &ReasoningServiceImpl,
    request: Request<DownloadWorkspaceFileRequest>,
) -> Result<Response<DownloadWorkspaceFileStream>, Status> {
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

    let file_path_str = req.path.clone();
    let cid = conv_id.to_string();

    // Resolve and validate path (blocking for canonicalize).
    let (canonical, content_type, total_size) =
        tokio::task::spawn_blocking(move || -> Result<(PathBuf, String, i64), String> {
            let ws_root = workspace_dir(&workspaces_path, &cid);
            let file_path = ws_root.join(&file_path_str);

            let canonical = file_path
                .canonicalize()
                .map_err(|_| "file not found".to_string())?;
            let canonical_root = ws_root
                .canonicalize()
                .map_err(|_| "workspace not found".to_string())?;

            if !canonical.starts_with(&canonical_root) {
                return Err("invalid path".to_string());
            }

            let metadata =
                std::fs::metadata(&canonical).map_err(|e| format!("metadata failed: {e}"))?;
            #[allow(clippy::cast_possible_wrap)]
            let total_size = metadata.len() as i64;

            let mut ct = guess_content_type(&file_path_str).to_string();
            if ct == "application/octet-stream"
                && std::fs::read(&canonical).is_ok_and(|s| looks_like_text(&s))
            {
                ct = "text/plain".to_string();
            }

            Ok((canonical, ct, total_size))
        })
        .await
        .map_err(|_| Status::internal("task failed"))?
        .map_err(Status::not_found)?;

    let (tx, rx) = tokio::sync::mpsc::channel(8);

    tokio::spawn(async move {
        let file = match tokio::fs::File::open(&canonical).await {
            Ok(f) => f,
            Err(e) => {
                let _ = tx
                    .send(Err(Status::not_found(format!("open failed: {e}"))))
                    .await;
                return;
            }
        };
        let mut reader = tokio::io::BufReader::new(file);
        let mut buf = vec![0u8; CHUNK_SIZE];
        let mut first = true;

        loop {
            match reader.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => {
                    let msg = DownloadWorkspaceFileResponse {
                        content_type: if first {
                            content_type.clone()
                        } else {
                            String::new()
                        },
                        total_size_bytes: if first { total_size } else { 0 },
                        data: buf.get(..n).unwrap_or_default().to_vec(),
                    };
                    first = false;
                    if tx.send(Ok(msg)).await.is_err() {
                        break; // client disconnected
                    }
                }
                Err(e) => {
                    let _ = tx
                        .send(Err(Status::internal(format!("read failed: {e}"))))
                        .await;
                    break;
                }
            }
        }
    });

    Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(
        rx,
    )))
}

fn write_upload_to_workspace(
    workspaces_path: &Path,
    conv_id: &str,
    path: &str,
    data: &[u8],
) -> Result<WorkspaceFileInfo, String> {
    let ws_root = workspace_dir(workspaces_path, conv_id);
    let file_path = ws_root.join(path);

    // Create parent directories (including the workspace root if it
    // does not exist yet — this supports uploading before the agent
    // container has started).
    if let Some(parent) = file_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directories: {e}"))?;
    }

    // Validate that the resolved path is inside the workspace root.
    // We canonicalize the parent (which now exists) and check containment.
    let parent = file_path
        .parent()
        .ok_or_else(|| "invalid file path".to_string())?;
    let canonical_parent = parent
        .canonicalize()
        .map_err(|e| format!("canonicalize parent failed: {e}"))?;
    let canonical_root = ws_root
        .canonicalize()
        .map_err(|e| format!("canonicalize root failed: {e}"))?;
    if !canonical_parent.starts_with(&canonical_root) {
        return Err("invalid path".to_string());
    }

    // Write to a temp file then rename for atomicity.
    let temp_path = file_path.with_extension("upload.tmp");
    std::fs::write(&temp_path, data).map_err(|e| format!("write failed: {e}"))?;
    std::fs::rename(&temp_path, &file_path).map_err(|e| format!("rename failed: {e}"))?;

    let metadata = std::fs::metadata(&file_path).map_err(|e| format!("metadata failed: {e}"))?;

    #[allow(clippy::cast_possible_wrap)]
    Ok(WorkspaceFileInfo {
        path: path.to_string(),
        size_bytes: metadata.len() as i64,
        is_directory: false,
        modified_unix: modified_time(&metadata),
        content_type: Some(guess_content_type(&file_path.to_string_lossy()).to_string()),
    })
}

pub async fn upload_workspace_file(
    svc: &ReasoningServiceImpl,
    request: Request<UploadWorkspaceFileRequest>,
) -> Result<Response<UploadWorkspaceFileResponse>, Status> {
    let _ctx = require_auth(&request)?;
    let req = request.into_inner();

    let conv_id: Uuid = req
        .conversation_id
        .parse()
        .map_err(|_| Status::invalid_argument("invalid conversation_id"))?;

    if req.path.is_empty() {
        return Err(Status::invalid_argument("path is required"));
    }
    if req.path.contains("..") || req.path.starts_with('/') {
        return Err(Status::invalid_argument("invalid path"));
    }
    if !req.path.starts_with("uploads/") {
        return Err(Status::invalid_argument(
            "uploaded files must be under uploads/",
        ));
    }
    if req.data.len() > MAX_UPLOAD_SIZE {
        return Err(Status::invalid_argument(format!(
            "file exceeds maximum size of {MAX_UPLOAD_SIZE} bytes"
        )));
    }

    let workspaces_path = svc
        .workspaces_path
        .as_ref()
        .ok_or_else(|| Status::unavailable("workspace storage not configured"))?
        .clone();

    let path = req.path.clone();
    let data = req.data;

    let file_info = tokio::task::spawn_blocking(move || {
        write_upload_to_workspace(&workspaces_path, &conv_id.to_string(), &path, &data)
    })
    .await
    .map_err(|_| Status::internal("task failed"))?
    .map_err(Status::internal)?;

    debug!(
        conversation_id = %conv_id,
        path = %file_info.path,
        size = file_info.size_bytes,
        "uploaded workspace file"
    );

    Ok(Response::new(UploadWorkspaceFileResponse {
        file: Some(file_info),
    }))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn upload_creates_file() {
        let tmp = TempDir::new().unwrap();
        let result =
            write_upload_to_workspace(tmp.path(), "conv-1", "uploads/report.pdf", b"hello world");
        let info = result.unwrap();
        assert_eq!(info.path, "uploads/report.pdf");
        assert_eq!(info.size_bytes, 11);
        assert!(!info.is_directory);

        let on_disk = std::fs::read(tmp.path().join("conv-1/uploads/report.pdf")).unwrap();
        assert_eq!(on_disk, b"hello world");
    }

    #[test]
    fn upload_creates_nested_directories() {
        let tmp = TempDir::new().unwrap();
        let result = write_upload_to_workspace(
            tmp.path(),
            "conv-2",
            "uploads/nested/deep/file.txt",
            b"content",
        );
        assert!(result.is_ok());
        assert!(
            tmp.path()
                .join("conv-2/uploads/nested/deep/file.txt")
                .exists()
        );
    }

    #[test]
    fn upload_overwrites_existing_file() {
        let tmp = TempDir::new().unwrap();
        write_upload_to_workspace(tmp.path(), "conv-3", "uploads/data.csv", b"old").unwrap();
        write_upload_to_workspace(tmp.path(), "conv-3", "uploads/data.csv", b"new").unwrap();

        let on_disk = std::fs::read(tmp.path().join("conv-3/uploads/data.csv")).unwrap();
        assert_eq!(on_disk, b"new");
    }

    #[test]
    fn upload_returns_correct_content_type() {
        let tmp = TempDir::new().unwrap();
        let info = write_upload_to_workspace(tmp.path(), "conv-4", "uploads/image.png", b"\x89PNG")
            .unwrap();
        assert_eq!(info.content_type.as_deref(), Some("image/png"));
    }

    #[test]
    fn guess_content_type_common_extensions() {
        assert_eq!(guess_content_type("file.json"), "application/json");
        assert_eq!(guess_content_type("file.py"), "text/x-python");
        assert_eq!(guess_content_type("file.rs"), "text/x-rust");
        assert_eq!(guess_content_type("file.pdf"), "application/pdf");
        assert_eq!(guess_content_type("Dockerfile"), "text/plain");
        assert_eq!(guess_content_type(".gitignore"), "text/plain");
    }
}
