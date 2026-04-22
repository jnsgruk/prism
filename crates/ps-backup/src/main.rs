use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use ps_core::backup::{BackupManifest, create_secret_key_canary};
use ps_core::crypto::load_secret_key;
use time::OffsetDateTime;
use tracing::{error, info, warn};

/// Database schemas to include in the `pg_dump`.
const SCHEMAS: &[&str] = &["config", "org", "activity", "metrics", "auth", "reasoning"];

/// Tables to exclude from the dump (currently none).
const EXCLUDED_TABLES: &[&str] = &[];

/// Directories to skip when walking workspace files (hidden dirs, build
/// artefacts, and filesystem-level directories like `lost+found`).
const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".cache",
    ".venv",
    ".opencode",
    ".mypy_cache",
    ".ruff_cache",
    "lost+found",
];

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();

    let mode = std::env::var("MODE").unwrap_or_else(|_| "backup".into());

    match mode.as_str() {
        "backup" => run_backup(),
        "restore" => run_restore(),
        other => bail!("unknown MODE: {other} (expected 'backup' or 'restore')"),
    }
}

// ---------------------------------------------------------------------------
// Backup mode
// ---------------------------------------------------------------------------

fn run_backup() -> Result<()> {
    let backup_id = std::env::var("BACKUP_ID").context("BACKUP_ID not set")?;
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let exclude_workspaces = std::env::var("EXCLUDE_WORKSPACES")
        .unwrap_or_else(|_| "false".into())
        .parse::<bool>()
        .unwrap_or(false);
    let workspaces_path = std::env::var("WORKSPACES_PATH").unwrap_or_else(|_| "/workspaces".into());
    let backups_path = std::env::var("BACKUPS_PATH").unwrap_or_else(|_| "/backups".into());

    info!(backup_id = %backup_id, "starting backup");

    let secret_key = load_secret_key()?;
    let canary = create_secret_key_canary(&secret_key)
        .map_err(|e| anyhow::anyhow!("failed to create canary: {e}"))?;

    // Get pg_dump version for manifest
    let pg_version = get_pg_version()?;
    info!(pg_version = %pg_version, "detected PostgreSQL client version");

    // Run pg_dump
    let dump_path = PathBuf::from("/tmp/database.dump");
    run_pg_dump(&database_url, &dump_path)?;
    info!("pg_dump completed successfully");

    // Walk workspaces first to get accurate counts
    let (workspace_file_count, workspace_total_bytes, workspace_files) = if exclude_workspaces {
        (0, 0i64, Vec::new())
    } else {
        walk_workspaces(Path::new(&workspaces_path))?
    };

    info!(
        workspace_file_count,
        workspace_total_bytes, "workspace scan complete"
    );

    // Build manifest
    let manifest = BackupManifest {
        format_version: 2,
        schema_version: 1,
        exported_at: OffsetDateTime::now_utc(),
        table_counts: BTreeMap::new(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        workspace_file_count,
        workspace_total_bytes,
        secret_key_canary: canary,
        pg_version,
        schemas: SCHEMAS.iter().map(|&s| s.to_owned()).collect(),
        exclude_workspaces,
    };

    // Build gzipped tar archive
    let tmp_output = PathBuf::from(&backups_path).join(format!("{backup_id}.ps-backup.tmp"));
    let final_output = PathBuf::from(&backups_path).join(format!("{backup_id}.ps-backup"));

    build_archive(
        &tmp_output,
        &manifest,
        &dump_path,
        &workspace_files,
        Path::new(&workspaces_path),
    )?;

    // Atomic rename
    std::fs::rename(&tmp_output, &final_output).context("failed to rename backup archive")?;

    // Clean up dump file
    let _ = std::fs::remove_file(&dump_path);

    info!(
        path = %final_output.display(),
        "backup completed successfully"
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Restore mode
// ---------------------------------------------------------------------------

fn run_restore() -> Result<()> {
    let backup_id = std::env::var("BACKUP_ID").context("BACKUP_ID not set")?;
    let database_url = std::env::var("DATABASE_URL").context("DATABASE_URL not set")?;
    let workspaces_path = std::env::var("WORKSPACES_PATH").unwrap_or_else(|_| "/workspaces".into());
    let backups_path = std::env::var("BACKUPS_PATH").unwrap_or_else(|_| "/backups".into());

    let archive_path = PathBuf::from(&backups_path).join(format!("{backup_id}.ps-backup"));
    info!(backup_id = %backup_id, archive = %archive_path.display(), "starting restore");

    if !archive_path.exists() {
        bail!("archive not found: {}", archive_path.display());
    }

    // 1. Ensure required extensions exist (pgvector lives in `public`,
    //    which is not part of the dump).
    ensure_extensions(&database_url)?;

    // 2. Drop application schemas so excluded tables (and their FKs)
    //    do not conflict with pg_restore.
    drop_schemas(&database_url)?;

    // 3. Extract database.dump and run pg_restore
    let dump_path = extract_database_dump(&archive_path)?;
    run_pg_restore(&database_url, &dump_path)?;
    let _ = std::fs::remove_file(&dump_path);
    info!("pg_restore completed successfully");

    // 4. Wipe existing workspace directories before restoring
    let wp = Path::new(&workspaces_path);
    wipe_workspace_dirs(wp);

    // 5. Extract workspace files from the archive
    extract_workspace_files(&archive_path, wp)?;

    // 6. Clean up the archive from the PVC
    if let Err(e) = std::fs::remove_file(&archive_path) {
        warn!(error = %e, "failed to clean up archive after restore");
    }

    info!("restore completed successfully");
    Ok(())
}

/// Ensure required `PostgreSQL` extensions exist before `pg_restore` runs.
fn ensure_extensions(database_url: &str) -> Result<()> {
    let output = std::process::Command::new("psql")
        .arg(database_url)
        .arg("-c")
        .arg("CREATE EXTENSION IF NOT EXISTS vector")
        .output()
        .context("failed to run psql for extension setup")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(stderr = %stderr, "failed to ensure pgvector extension");
        bail!("failed to ensure pgvector extension: {stderr}");
    }

    info!("pgvector extension ensured");
    Ok(())
}

/// Drop all application schemas before `pg_restore` so that the restore
/// starts from a clean slate. `pg_restore` will recreate the schemas from
/// the dump.
fn drop_schemas(database_url: &str) -> Result<()> {
    let sql = SCHEMAS
        .iter()
        .map(|schema| format!("DROP SCHEMA IF EXISTS {schema} CASCADE;"))
        .collect::<Vec<_>>()
        .join("\n");

    let output = std::process::Command::new("psql")
        .arg(database_url)
        .arg("-c")
        .arg(&sql)
        .output()
        .context("failed to run psql to drop schemas")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(stderr = %stderr, "failed to drop schemas before restore");
        bail!("failed to drop schemas: {stderr}");
    }

    info!("application schemas dropped");
    Ok(())
}

/// Extract `database.dump` from the gzipped tar archive into a temp file.
fn extract_database_dump(archive_path: &Path) -> Result<PathBuf> {
    let dump_path = PathBuf::from("/tmp/database.dump");
    let file = std::fs::File::open(archive_path)
        .with_context(|| format!("failed to open archive: {}", archive_path.display()))?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    for entry in archive
        .entries()
        .context("failed to read archive entries")?
    {
        let mut entry = entry.context("failed to read archive entry")?;
        let path = entry.path().context("invalid entry path")?;
        if path.to_string_lossy() == "database.dump" {
            let mut out =
                std::fs::File::create(&dump_path).context("failed to create temp dump file")?;
            std::io::copy(&mut entry, &mut out).context("failed to extract database.dump")?;
            return Ok(dump_path);
        }
    }

    bail!("archive does not contain database.dump");
}

/// Run `pg_restore` against the database.
///
/// Schemas are dropped before this call, so `--clean` is not needed.
fn run_pg_restore(database_url: &str, dump_path: &Path) -> Result<()> {
    let output = std::process::Command::new("pg_restore")
        .arg("--dbname")
        .arg(database_url)
        .arg("--single-transaction")
        .arg("--no-owner")
        .arg("--no-privileges")
        .arg(dump_path)
        .output()
        .context("failed to execute pg_restore")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(stderr = %stderr, "pg_restore failed");
        bail!("pg_restore exited with status {}: {stderr}", output.status);
    }

    Ok(())
}

/// Wipe existing workspace directories before restoring.
fn wipe_workspace_dirs(workspaces_path: &Path) {
    match std::fs::read_dir(workspaces_path) {
        Ok(entries) => {
            let mut removed = 0usize;
            for entry in entries.flatten() {
                if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
                    if let Err(e) = std::fs::remove_dir_all(entry.path()) {
                        warn!(
                            path = %entry.path().display(),
                            error = %e,
                            "failed to remove workspace dir during restore wipe"
                        );
                    } else {
                        removed += 1;
                    }
                }
            }
            info!(
                count = removed,
                "workspace directories wiped before restore"
            );
        }
        Err(e) => {
            warn!(error = %e, "could not read workspaces_path during restore wipe");
        }
    }
}

/// Extract workspace files from the archive.
fn extract_workspace_files(archive_path: &Path, workspaces_path: &Path) -> Result<()> {
    let file = std::fs::File::open(archive_path)
        .context("failed to open archive for workspace extraction")?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);

    for entry in archive
        .entries()
        .context("failed to read archive entries")?
    {
        let mut entry = entry.context("failed to read archive entry")?;
        let path = entry.path().context("invalid entry path")?;
        let path_str = path.to_string_lossy().to_string();

        if !path_str.starts_with("workspaces/") {
            continue;
        }

        restore_workspace_file(&mut entry, &path_str, workspaces_path)?;
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

/// Parse the Postgres major version from `pg_dump --version`.
fn get_pg_version() -> Result<String> {
    let output = std::process::Command::new("pg_dump")
        .arg("--version")
        .output()
        .context("failed to run pg_dump --version")?;

    if !output.status.success() {
        bail!("pg_dump --version failed");
    }

    let version_str = String::from_utf8_lossy(&output.stdout);
    // Output is like "pg_dump (PostgreSQL) 17.2"
    let version = version_str
        .split_whitespace()
        .last()
        .unwrap_or("unknown")
        .split('.')
        .next()
        .unwrap_or("unknown");

    Ok(version.to_owned())
}

/// Run `pg_dump` with the specified schemas and exclusions.
fn run_pg_dump(database_url: &str, output_path: &Path) -> Result<()> {
    let mut cmd = std::process::Command::new("pg_dump");
    cmd.arg("--format=custom")
        .arg("--no-owner")
        .arg("--no-privileges");

    for schema in SCHEMAS {
        cmd.arg(format!("--schema={schema}"));
    }

    for table in EXCLUDED_TABLES {
        cmd.arg(format!("--exclude-table={table}"));
    }

    cmd.arg(format!("--file={}", output_path.display()))
        .arg(database_url);

    let output = cmd.output().context("failed to execute pg_dump")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        error!(stderr = %stderr, "pg_dump failed");
        bail!("pg_dump exited with status {}: {stderr}", output.status);
    }

    Ok(())
}

/// Walk the workspaces directory, collecting file paths and counts.
///
/// Returns `(file_count, total_bytes, files)` where files is a vec of
/// `(relative_path, absolute_path)` pairs.
#[allow(clippy::type_complexity)]
fn walk_workspaces(workspaces_path: &Path) -> Result<(i32, i64, Vec<(String, PathBuf)>)> {
    let mut files = Vec::new();
    let mut total_bytes: i64 = 0;

    if !workspaces_path.exists() {
        return Ok((0, 0, files));
    }

    walk_dir_recursive(
        workspaces_path,
        workspaces_path,
        &mut files,
        &mut total_bytes,
    )?;

    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
    let count = files.len() as i32;
    Ok((count, total_bytes, files))
}

/// Recursively walk a directory, collecting files and their relative paths.
fn walk_dir_recursive(
    base: &Path,
    current: &Path,
    files: &mut Vec<(String, PathBuf)>,
    total_bytes: &mut i64,
) -> Result<()> {
    let entries = match std::fs::read_dir(current) {
        Ok(entries) => entries,
        Err(e) => {
            tracing::warn!(path = %current.display(), error = %e, "failed to read directory");
            return Ok(());
        }
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        if path.is_dir() {
            if SKIP_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk_dir_recursive(base, &path, files, total_bytes)?;
        } else if path.is_file() {
            let rel_path = path
                .strip_prefix(base)
                .context("failed to compute relative path")?;
            let rel_str = format!("workspaces/{}", rel_path.display());

            let metadata = std::fs::metadata(&path)?;
            *total_bytes += metadata.len().cast_signed();
            files.push((rel_str, path));
        }
    }

    Ok(())
}

/// Build the gzipped tar archive.
fn build_archive(
    output_path: &Path,
    manifest: &BackupManifest,
    dump_path: &Path,
    workspace_files: &[(String, PathBuf)],
    _workspaces_path: &Path,
) -> Result<()> {
    let file = std::fs::File::create(output_path).context("failed to create archive file")?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);

    // 1. Write manifest.json
    let manifest_json =
        serde_json::to_vec_pretty(manifest).context("failed to serialize manifest")?;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest_json.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "manifest.json", manifest_json.as_slice())
        .context("failed to write manifest.json to archive")?;

    // 2. Write database.dump
    let dump_file = std::fs::File::open(dump_path).context("failed to open database.dump")?;
    let dump_size = dump_file.metadata()?.len();
    let mut header = tar::Header::new_gnu();
    header.set_size(dump_size);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(
        &mut header,
        "database.dump",
        std::io::BufReader::new(dump_file),
    )
    .context("failed to write database.dump to archive")?;

    info!(
        dump_size_mb = dump_size / (1024 * 1024),
        "database dump added to archive"
    );

    // 3. Write workspace files
    for (rel_path, abs_path) in workspace_files {
        let ws_file = match std::fs::File::open(abs_path) {
            Ok(f) => f,
            Err(e) => {
                tracing::warn!(path = %abs_path.display(), error = %e, "skipping unreadable workspace file");
                continue;
            }
        };
        let ws_size = ws_file.metadata()?.len();
        let mut header = tar::Header::new_gnu();
        header.set_size(ws_size);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, rel_path, std::io::BufReader::new(ws_file))
            .with_context(|| format!("failed to write {rel_path} to archive"))?;
    }

    // Finalize
    let encoder = tar.into_inner().context("failed to finalize tar")?;
    encoder.finish().context("failed to finish gzip")?;

    Ok(())
}

/// Restore a single workspace file entry from the backup archive.
///
/// Entry name format: `workspaces/<conv-uuid>/<relative-path>`
///
/// Security:
/// - Rejects relative paths with `..` or absolute paths
/// - Canonicalizes target parent and verifies it is inside `workspaces_path`
fn restore_workspace_file(
    entry: &mut dyn std::io::Read,
    name: &str,
    workspaces_path: &Path,
) -> Result<()> {
    // Parse: workspaces/<conv-uuid>/<relative-path>
    let without_prefix = name.strip_prefix("workspaces/").unwrap_or(name);
    let slash = without_prefix
        .find('/')
        .with_context(|| format!("malformed workspace entry (no subpath): {name}"))?;
    let (conv_id_str, rel_path) = without_prefix.split_at(slash);
    let rel_path = rel_path.trim_start_matches('/');

    // Validate UUID format
    let _conv_id: uuid::Uuid = conv_id_str
        .parse()
        .with_context(|| format!("workspace entry has invalid UUID: {name}"))?;

    // Reject absolute paths and path traversal
    if rel_path.is_empty() || rel_path.starts_with('/') {
        bail!("workspace entry has invalid relative path: {name}");
    }
    for component in Path::new(rel_path).components() {
        if matches!(component, std::path::Component::ParentDir) {
            bail!("workspace entry contains path traversal: {name}");
        }
    }

    let target = workspaces_path.join(conv_id_str).join(rel_path);

    // Canonicalize parent and verify it stays within workspaces_path
    let parent = target
        .parent()
        .with_context(|| format!("workspace entry has no parent dir: {name}"))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create workspace dir {}", parent.display()))?;
    let canonical_parent = parent
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", parent.display()))?;
    let canonical_wp = workspaces_path
        .canonicalize()
        .context("failed to canonicalize workspaces_path")?;
    if !canonical_parent.starts_with(&canonical_wp) {
        bail!("workspace entry escapes workspaces_path: {name}");
    }

    // Stream content to disk
    let mut file = std::fs::File::create(&target)
        .with_context(|| format!("failed to create workspace file {}", target.display()))?;

    // Set standard permissions (0o644)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o644));
    }

    std::io::copy(entry, &mut file)
        .with_context(|| format!("failed to write workspace file {}", target.display()))?;

    Ok(())
}
