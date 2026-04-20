use std::collections::BTreeMap;
use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use tracing::warn;
use uuid::Uuid;

use crate::Error;

// ---------------------------------------------------------------------------
// Helper: base64 encode/decode for BYTEA columns stored in JSONL
// ---------------------------------------------------------------------------

/// Base64-encode a byte slice using the standard alphabet.
pub fn base64_encode(data: &[u8]) -> String {
    BASE64_ENGINE.encode(data)
}

/// Base64-decode a string.
pub fn base64_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    BASE64_ENGINE.decode(s)
}

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;

// ---------------------------------------------------------------------------
// Backup row types — plain-data structs used for JSONL serialisation and
// deserialisation.  One struct per table that participates in backup/restore.
// ---------------------------------------------------------------------------

/// A row from `config.secrets`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SecretRow {
    pub id: Uuid,
    pub source_id: Option<Uuid>,
    pub secret_key: String,
    /// Base64-encoded ciphertext (BYTEA round-trips via JSON as base64).
    pub encrypted_value: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// A row from `config.global_settings`.
#[derive(Debug, Serialize, Deserialize)]
pub struct GlobalSettingRow {
    pub key: String,
    pub value: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

/// A row from `org.platform_identities`.
#[derive(Debug, Serialize, Deserialize)]
pub struct PlatformIdentityRow {
    pub id: Uuid,
    pub person_id: Uuid,
    pub platform: String,
    pub platform_username: String,
    pub platform_user_id: Option<String>,
}

/// A row from `org.team_memberships`.
#[derive(Debug, Serialize, Deserialize)]
pub struct TeamMembershipRow {
    pub id: Uuid,
    pub person_id: Uuid,
    pub team_id: Uuid,
    pub start_date: String, // ISO date string
    pub end_date: Option<String>,
}

/// A row from `org.repositories`.
#[derive(Debug, Serialize, Deserialize)]
pub struct RepositoryRow {
    pub id: Uuid,
    pub github_org: String,
    pub github_repo: String,
    pub default_branch: Option<String>,
    pub primary_language: Option<String>,
    pub team_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A row from `activity.ingestion_watermarks`.
#[derive(Debug, Serialize, Deserialize)]
pub struct WatermarkRow {
    pub source_name: String,
    pub watermark_value: String,
    pub last_successful_run: Option<String>,
    pub last_attempt: Option<String>,
    pub last_error: Option<String>,
    pub items_collected_last_run: Option<i32>,
}

/// A row from `activity.contributions`.
#[derive(Debug, Serialize, Deserialize)]
pub struct ContributionRow {
    pub id: Uuid,
    pub person_id: Option<Uuid>,
    pub platform: String,
    pub contribution_type: String,
    pub platform_id: String,
    pub title: Option<String>,
    pub url: Option<String>,
    pub state: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub updated_at: Option<String>,
    pub closed_at: Option<String>,
    pub metrics: serde_json::Value,
    pub metadata: serde_json::Value,
    pub content: Option<String>,
    pub state_history: Option<serde_json::Value>,
    #[serde(with = "time::serde::rfc3339")]
    pub ingested_at: OffsetDateTime,
}

/// A row from `reasoning.enrichments`.
#[derive(Debug, Serialize, Deserialize)]
pub struct EnrichmentRow {
    pub id: Uuid,
    pub contribution_id: Uuid,
    pub enrichment_type: String,
    pub value: serde_json::Value,
    pub model_name: String,
    pub confidence: Option<f32>,
    pub input_hash: Option<String>,
    pub input_preview: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A row from `reasoning.embeddings`.
#[derive(Debug, Serialize, Deserialize)]
pub struct EmbeddingRow {
    pub id: Uuid,
    pub contribution_id: Uuid,
    /// The 768-dimensional vector stored as a JSON array of f32.
    pub embedding: Vec<f32>,
    pub model_name: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

/// A row from `reasoning.insight_snapshots`.
#[derive(Debug, Serialize, Deserialize)]
pub struct InsightSnapshotRow {
    pub id: Uuid,
    pub team_id: Uuid,
    pub period_start: String,
    pub period_end: String,
    pub period_type: String,
    pub avg_review_depth: Option<f64>,
    pub rubber_stamp_pct: Option<f64>,
    pub deep_review_pct: Option<f64>,
    pub depth_distribution: Vec<i32>,
    pub constructive_count: i32,
    pub neutral_count: i32,
    pub critical_count: i32,
    pub hostile_count: i32,
    pub significant_count: i32,
    pub notable_count: i32,
    pub routine_count: i32,
    pub enrichment_coverage: serde_json::Value,
    pub raw_insights: serde_json::Value,
    pub conversation_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub computed_at: OffsetDateTime,
}

/// A row from `reasoning.insight_snapshot_sources`.
#[derive(Debug, Serialize, Deserialize)]
pub struct InsightSnapshotSourceRow {
    pub snapshot_id: Uuid,
    pub enrichment_id: Uuid,
}

/// A row from `metrics.team_snapshots` (backup format).
#[derive(Debug, Serialize, Deserialize)]
pub struct TeamSnapshotBackupRow {
    pub id: Uuid,
    pub team_id: Uuid,
    pub period_start: String,
    pub period_end: String,
    pub period_type: String,
    pub throughput: Option<i32>,
    pub avg_review_turnaround_hours: Option<f64>,
    pub deployment_frequency: Option<f64>,
    pub lead_time_hours: Option<f64>,
    pub change_failure_rate: Option<f64>,
    pub mttr_hours: Option<f64>,
    pub avg_cycle_time_hours: Option<f64>,
    pub wip_avg: Option<f64>,
    pub flow_efficiency: Option<f64>,
    pub avg_review_depth: Option<f64>,
    pub raw_metrics: Option<serde_json::Value>,
    #[serde(with = "time::serde::rfc3339")]
    pub computed_at: OffsetDateTime,
}

/// A row from `metrics.individual_profiles`.
#[derive(Debug, Serialize, Deserialize)]
pub struct IndividualProfileRow {
    pub id: Uuid,
    pub person_id: Uuid,
    pub period_start: String,
    pub period_end: String,
    pub period_type: String,
    pub activity_summary: serde_json::Value,
    pub peer_comparison: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    pub computed_at: OffsetDateTime,
}

/// A row from `metrics.snapshot_sources`.
#[derive(Debug, Serialize, Deserialize)]
pub struct SnapshotSourceRow {
    pub snapshot_id: Uuid,
    pub contribution_id: Uuid,
}

/// A row from `auth.users` including the password hash (backup only).
#[derive(Debug, Serialize, Deserialize)]
pub struct UserBackupRow {
    pub id: Uuid,
    pub username: String,
    pub display_name: String,
    /// bcrypt/argon2 password hash — included in backup for faithful restore.
    pub password_hash: String,
    pub role: String,
    pub is_active: bool,
    pub person_id: Option<Uuid>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: time::OffsetDateTime,
}

/// A row from `reasoning.conversations` (backup format).
#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationBackupRow {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: Option<String>,
    pub status: String,
    pub model_name: String,
    pub container_pod_name: Option<String>,
    pub container_status: String,
    pub opencode_session_id: Option<String>,
    pub container_pod_ip: Option<String>,
    pub total_tool_calls: i32,
    pub total_prompt_tokens: i32,
    pub total_completion_tokens: i32,
    pub query_status: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub last_activity_at: time::OffsetDateTime,
}

/// A row from `reasoning.conversation_messages` (backup format).
#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationMessageBackupRow {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub role: String,
    pub content: String,
    pub reasoning_trace: Option<serde_json::Value>,
    pub supporting_data: Option<serde_json::Value>,
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub attached_files: Vec<String>,
    pub mentions: serde_json::Value,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: time::OffsetDateTime,
}

/// The current backup format version. Increment when the set of tables or
/// their serialised shapes change incompatibly.
pub const SCHEMA_VERSION: i32 = 6;

/// Metadata about a backup archive, stored as the first entry in the tar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub schema_version: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub exported_at: OffsetDateTime,
    pub table_counts: BTreeMap<String, i32>,
    pub app_version: String,
}

/// Validate the schema version of an incoming backup against the current code.
///
/// - version == current  → `Ok(())`
/// - version < current   → `Ok(())` with a warning logged (best-effort restore)
/// - version > current   → `Err(...)` (backup was made with a newer Prism; refuse)
pub fn check_schema_version(version: i32) -> Result<(), Error> {
    match version.cmp(&SCHEMA_VERSION) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Less => {
            warn!(
                backup_version = version,
                current_version = SCHEMA_VERSION,
                "restoring backup from an older schema version; some fields may be missing"
            );
            Ok(())
        }
        std::cmp::Ordering::Greater => Err(Error::Backup(format!(
            "backup schema version {version} is newer than this Prism installation \
             (version {SCHEMA_VERSION}); upgrade Prism before restoring"
        ))),
    }
}

/// Writes a `.ps-backup` archive (gzipped tar with manifest + JSONL tables).
pub struct BackupWriter<W: Write> {
    builder: tar::Builder<GzEncoder<W>>,
}

impl<W: Write> BackupWriter<W> {
    pub fn new(writer: W) -> Self {
        let encoder = GzEncoder::new(writer, Compression::default());
        Self {
            builder: tar::Builder::new(encoder),
        }
    }

    /// Write the manifest as the first entry in the archive.
    pub fn write_manifest(&mut self, manifest: &BackupManifest) -> Result<(), Error> {
        let data = serde_json::to_vec_pretty(manifest)
            .map_err(|e| Error::Backup(format!("failed to serialize manifest: {e}")))?;
        self.write_entry("manifest.json", &data)
    }

    /// Write a table's rows as a JSONL entry in the archive.
    pub fn write_table<T: Serialize>(&mut self, table_name: &str, rows: &[T]) -> Result<(), Error> {
        let mut data = Vec::with_capacity(rows.len() * 256);
        for row in rows {
            serde_json::to_writer(&mut data, row).map_err(|e| {
                Error::Backup(format!("failed to serialize row in {table_name}: {e}"))
            })?;
            data.push(b'\n');
        }
        self.write_entry(&format!("{table_name}.jsonl"), &data)
    }

    /// Finish writing the archive and flush all data.
    pub fn finish(mut self) -> Result<(), Error> {
        self.builder
            .finish()
            .map_err(|e| Error::Backup(format!("failed to finish archive: {e}")))?;
        self.builder
            .into_inner()
            .map_err(|e| Error::Backup(format!("failed to flush archive: {e}")))?
            .finish()
            .map_err(|e| Error::Backup(format!("failed to finish gzip: {e}")))?;
        Ok(())
    }

    fn write_entry(&mut self, name: &str, data: &[u8]) -> Result<(), Error> {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();

        self.builder
            .append_data(&mut header, name, data)
            .map_err(|e| Error::Backup(format!("failed to write {name}: {e}")))
    }
}

/// Reads a `.ps-backup` archive, providing sequential access to its entries.
///
/// The archive must be read in order: manifest first, then table entries in
/// the order they were written. The `entries` iterator is consumed as you go.
pub struct BackupReader<R: Read> {
    archive: tar::Archive<GzDecoder<R>>,
}

impl<R: Read> BackupReader<R> {
    pub fn new(reader: R) -> Self {
        let decoder = GzDecoder::new(reader);
        Self {
            archive: tar::Archive::new(decoder),
        }
    }

    /// Read the manifest from the archive. Must be called first.
    pub fn read_manifest(&mut self) -> Result<BackupManifest, Error> {
        let mut entries = self
            .archive
            .entries()
            .map_err(|e| Error::Backup(format!("failed to read archive: {e}")))?;

        let entry = entries
            .next()
            .ok_or_else(|| Error::Backup("archive is empty".into()))?
            .map_err(|e| Error::Backup(format!("failed to read manifest entry: {e}")))?;

        serde_json::from_reader(entry)
            .map_err(|e| Error::Backup(format!("failed to parse manifest: {e}")))
    }

    /// Read all entries after the manifest, calling `visitor` for each one.
    ///
    /// `visitor` receives the entry name (e.g. `"people.jsonl"`) and the raw
    /// bytes of that entry. Entries are visited in archive order.
    pub fn read_entries<F>(&mut self, mut visitor: F) -> Result<(), Error>
    where
        F: FnMut(&str, Vec<u8>) -> Result<(), Error>,
    {
        let entries = self
            .archive
            .entries()
            .map_err(|e| Error::Backup(format!("failed to read archive entries: {e}")))?;

        for entry in entries {
            let mut entry =
                entry.map_err(|e| Error::Backup(format!("failed to read archive entry: {e}")))?;

            let name = entry
                .path()
                .map_err(|e| Error::Backup(format!("failed to read entry path: {e}")))?
                .to_string_lossy()
                .into_owned();

            // Skip the manifest — callers use read_manifest() for that.
            if name == "manifest.json" {
                continue;
            }

            let mut data = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut data)
                .map_err(|e| Error::Backup(format!("failed to read entry {name}: {e}")))?;

            visitor(&name, data)?;
        }

        Ok(())
    }
}

/// Read a JSONL table entry from raw bytes, deserializing each line.
pub fn read_table_rows<T: for<'de> Deserialize<'de>>(data: &[u8]) -> Result<Vec<T>, Error> {
    let mut rows = Vec::new();
    for line in data.split(|&b| b == b'\n') {
        if line.is_empty() {
            continue;
        }
        let row: T = serde_json::from_slice(line)
            .map_err(|e| Error::Backup(format!("failed to parse row: {e}")))?;
        rows.push(row);
    }
    Ok(rows)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::items_after_statements)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip() {
        let manifest = BackupManifest {
            schema_version: SCHEMA_VERSION,
            exported_at: OffsetDateTime::now_utc(),
            table_counts: BTreeMap::from([("users".into(), 3), ("teams".into(), 2)]),
            app_version: "0.1.0".into(),
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: BackupManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.table_counts.len(), 2);
    }

    #[test]
    fn write_and_read_archive() {
        let manifest = BackupManifest {
            schema_version: SCHEMA_VERSION,
            exported_at: OffsetDateTime::now_utc(),
            table_counts: BTreeMap::from([("test_table".into(), 2)]),
            app_version: "0.1.0".into(),
        };

        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Row {
            id: i32,
            name: String,
        }

        let rows = vec![
            Row {
                id: 1,
                name: "Alice".into(),
            },
            Row {
                id: 2,
                name: "Bob".into(),
            },
        ];

        // Write
        let mut buf = Vec::new();
        let mut writer = BackupWriter::new(&mut buf);
        writer.write_manifest(&manifest).unwrap();
        writer.write_table("test_table", &rows).unwrap();
        writer.finish().unwrap();

        // Read manifest
        let mut reader = BackupReader::new(buf.as_slice());
        let read_manifest = reader.read_manifest().unwrap();
        assert_eq!(read_manifest.schema_version, SCHEMA_VERSION);
        assert_eq!(read_manifest.app_version, "0.1.0");
    }

    #[test]
    fn schema_version_check() {
        assert!(check_schema_version(SCHEMA_VERSION).is_ok());
        assert!(check_schema_version(SCHEMA_VERSION - 1).is_ok()); // older → warn but ok
        assert!(check_schema_version(SCHEMA_VERSION + 1).is_err()); // newer → reject
    }

    #[test]
    fn jsonl_roundtrip() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Item {
            value: String,
        }

        let items = vec![
            Item {
                value: "one".into(),
            },
            Item {
                value: "two".into(),
            },
        ];

        let mut data = Vec::new();
        for item in &items {
            serde_json::to_writer(&mut data, item).unwrap();
            data.push(b'\n');
        }

        let parsed: Vec<Item> = read_table_rows(&data).unwrap();
        assert_eq!(parsed, items);
    }

    #[test]
    fn read_entries_visits_non_manifest() {
        let manifest = BackupManifest {
            schema_version: SCHEMA_VERSION,
            exported_at: OffsetDateTime::now_utc(),
            table_counts: BTreeMap::new(),
            app_version: "0.1.0".into(),
        };

        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Row {
            x: i32,
        }

        let rows = vec![Row { x: 1 }, Row { x: 2 }];

        let mut buf = Vec::new();
        let mut writer = BackupWriter::new(&mut buf);
        writer.write_manifest(&manifest).unwrap();
        writer.write_table("things", &rows).unwrap();
        writer.finish().unwrap();

        let mut reader = BackupReader::new(buf.as_slice());
        let mut visited = Vec::new();
        reader
            .read_entries(|name, data| {
                visited.push((name.to_owned(), data));
                Ok(())
            })
            .unwrap();

        assert_eq!(visited.len(), 1);
        assert_eq!(visited[0].0, "things.jsonl");
        let parsed: Vec<Row> = read_table_rows(&visited[0].1).unwrap();
        assert_eq!(parsed, rows);
    }
}
