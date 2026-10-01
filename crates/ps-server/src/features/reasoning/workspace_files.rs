//! Shared, confined workspace-file resolution. RPC paths are already decoded.
use std::{fs::File, io::Read, path::Path};
use tonic::Status;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileError {
    Missing,
    Invalid,
    Unavailable,
}

impl FileError {
    pub(crate) fn status(self) -> Status {
        match self {
            Self::Missing => Status::not_found("file not found"),
            Self::Invalid => Status::invalid_argument("invalid workspace file"),
            Self::Unavailable => Status::unavailable("workspace file unavailable"),
        }
    }
}

pub(crate) struct ResolvedFile {
    pub file: File,
    pub content_type: String,
    pub size_bytes: i64,
}

pub(crate) fn valid_relative_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.chars().any(|c| c.is_control() || c == '\\')
        && path
            .split('/')
            .all(|segment| !matches!(segment, "" | "." | ".."))
}

fn filesystem_error(error: std::io::Error) -> FileError {
    tracing::warn!(error = %error, "workspace filesystem operation failed");
    match error.kind() {
        std::io::ErrorKind::NotFound => FileError::Missing,
        _ => FileError::Unavailable,
    }
}

pub(crate) fn resolve_file(
    root: &Path,
    conversation: &str,
    path: &str,
) -> Result<ResolvedFile, FileError> {
    if uuid::Uuid::parse_str(conversation).is_err() || !valid_relative_path(path) {
        return Err(FileError::Invalid);
    }
    // An absent storage mount is an outage; an absent conversation directory is missing.
    let storage = root.canonicalize().map_err(|e| {
        tracing::warn!(error = %e, "workspace storage unavailable");
        FileError::Unavailable
    })?;
    let workspace = storage
        .join(conversation)
        .canonicalize()
        .map_err(filesystem_error)?;
    if !workspace.starts_with(&storage) {
        return Err(FileError::Invalid);
    }
    let target = workspace
        .join(path)
        .canonicalize()
        .map_err(filesystem_error)?;
    if !target.starts_with(&workspace) {
        return Err(FileError::Invalid);
    }
    if !std::fs::metadata(&target)
        .map_err(filesystem_error)?
        .is_file()
    {
        return Err(FileError::Invalid);
    }
    // Retain the opened handle for transfer, avoiding a second open after validation.
    let mut file = open_confined(&storage, conversation, &workspace, &target)?;
    let metadata = file.metadata().map_err(filesystem_error)?;
    if !metadata.is_file() {
        return Err(FileError::Invalid);
    }
    let mut content_type = guess_content_type(path).to_string();
    if content_type == "application/octet-stream" {
        let mut prefix = [0; 8192];
        let count = file.read(&mut prefix).map_err(filesystem_error)?;
        let sample = prefix.get(..count).ok_or(FileError::Unavailable)?;
        if !sample.contains(&0) && std::str::from_utf8(sample).is_ok() {
            content_type = "text/plain".to_string();
        }
        std::io::Seek::rewind(&mut file).map_err(filesystem_error)?;
    }
    Ok(ResolvedFile {
        file,
        content_type,
        size_bytes: i64::try_from(metadata.len()).map_err(|_| FileError::Unavailable)?,
    })
}
fn open_confined(
    storage: &Path,
    conversation: &str,
    workspace: &Path,
    target: &Path,
) -> Result<File, FileError> {
    use nix::fcntl::{OFlag, openat};
    use nix::sys::stat::Mode;
    let storage_handle = File::open(storage).map_err(filesystem_error)?;
    let flags = OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW;
    let mut directory = openat(
        &storage_handle,
        conversation,
        flags | OFlag::O_DIRECTORY,
        Mode::empty(),
    )
    .map_err(|e| filesystem_error(e.into()))?;
    let relative = target
        .strip_prefix(workspace)
        .map_err(|_| FileError::Invalid)?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let component_flags = if components.peek().is_some() {
            flags | OFlag::O_DIRECTORY
        } else {
            flags | OFlag::O_NONBLOCK
        };
        directory = openat(
            &directory,
            component.as_os_str(),
            component_flags,
            Mode::empty(),
        )
        .map_err(|e| filesystem_error(e.into()))?;
    }
    Ok(File::from(directory))
}

pub(crate) fn guess_content_type(filename: &str) -> &'static str {
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn resolution_confines_files_and_distinguishes_failures() {
        let temp = tempfile::tempdir().unwrap();
        let conversation = uuid::Uuid::new_v4().to_string();
        let root = temp.path().join(&conversation);
        std::fs::create_dir_all(root.join("nested")).unwrap();
        std::fs::write(root.join("nested/é % # ?.pdf"), b"%PDF-test").unwrap();
        let file = resolve_file(temp.path(), &conversation, "nested/é % # ?.pdf").unwrap();
        assert_eq!(file.size_bytes, 9);
        assert_eq!(file.content_type, "application/pdf");
        assert_eq!(
            resolve_file(temp.path(), &conversation, "nested").err(),
            Some(FileError::Invalid)
        );
        assert_eq!(
            resolve_file(temp.path(), &conversation, "missing").err(),
            Some(FileError::Missing)
        );
        for path in ["../escape", "/etc/passwd", "a/../b", "a//b", "a\\b"] {
            assert_eq!(
                resolve_file(temp.path(), &conversation, path).err(),
                Some(FileError::Invalid)
            );
        }
        assert_eq!(
            resolve_file(&temp.path().join("unmounted"), &conversation, "file").err(),
            Some(FileError::Unavailable)
        );
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(temp.path(), root.join("escape")).unwrap();
            assert_eq!(
                resolve_file(temp.path(), &conversation, "escape/outside").err(),
                Some(FileError::Missing)
            );
            std::fs::write(temp.path().join("outside"), b"outside").unwrap();
            assert_eq!(
                resolve_file(temp.path(), &conversation, "escape/outside").err(),
                Some(FileError::Invalid)
            );
        }
    }
}
