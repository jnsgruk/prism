use std::collections::BTreeMap;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_ENGINE;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::Error;
use crate::crypto;

/// Base64-encode a byte slice using the standard alphabet.
fn base64_encode(data: &[u8]) -> String {
    BASE64_ENGINE.encode(data)
}

/// Base64-decode a string.
fn base64_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    BASE64_ENGINE.decode(s)
}

/// The current backup format version. Reset to 1 — there are no real-world
/// backups at any prior schema version.
pub const SCHEMA_VERSION: i32 = 1;

/// Name of the tar entry that holds the hex-encoded SHA-256 integrity checksum.
pub const CHECKSUM_ENTRY_NAME: &str = "checksum.sha256";

/// Known plaintext used for the secret key canary.
const CANARY_PLAINTEXT: &[u8] = b"prism-backup-key-check";

/// Metadata about a backup archive, stored as the first entry in the tar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    /// Archive format version: 1 = legacy JSONL, 2 = `pg_dump` custom format.
    #[serde(default)]
    pub format_version: i32,
    pub schema_version: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub exported_at: OffsetDateTime,
    pub table_counts: BTreeMap<String, i32>,
    pub app_version: String,
    /// Number of workspace files included in the backup.
    pub workspace_file_count: i32,
    /// Total size of workspace files in bytes.
    pub workspace_total_bytes: i64,
    /// Base64-encoded AES-256-GCM encrypted canary for `PS_SECRET_KEY` validation.
    pub secret_key_canary: String,
    /// Postgres major version used for `pg_dump` (v2 only).
    #[serde(default)]
    pub pg_version: String,
    /// Database schemas included in the dump (v2 only).
    #[serde(default)]
    pub schemas: Vec<String>,
    /// Whether workspace files were excluded from the backup (v2 only).
    #[serde(default)]
    pub exclude_workspaces: bool,
}

/// Validate the schema version of an incoming backup against the current code.
pub fn check_schema_version(version: i32) -> Result<(), Error> {
    if version == SCHEMA_VERSION {
        Ok(())
    } else {
        Err(Error::Backup(format!(
            "backup schema version {version} does not match this Prism installation \
             (version {SCHEMA_VERSION}); use a matching Prism version"
        )))
    }
}

/// Encrypt the canary using the same AES-256-GCM encryption used for secrets.
pub fn create_secret_key_canary(secret_key: &[u8; 32]) -> Result<String, Error> {
    let encrypted = crypto::encrypt(secret_key, CANARY_PLAINTEXT)?;
    Ok(base64_encode(&encrypted))
}

/// Validate the canary. Returns `Ok(())` if valid, `Err` if key mismatch.
pub fn validate_secret_key_canary(canary: &str, secret_key: &[u8; 32]) -> Result<(), Error> {
    let encrypted =
        base64_decode(canary).map_err(|e| Error::Backup(format!("invalid canary base64: {e}")))?;
    let decrypted = crypto::decrypt(secret_key, &encrypted).map_err(|_| {
        Error::Backup(
            "PS_SECRET_KEY mismatch: cannot decrypt canary. \
             The backup was created with a different encryption key."
                .into(),
        )
    })?;
    if decrypted == CANARY_PLAINTEXT {
        Ok(())
    } else {
        Err(Error::Backup(
            "PS_SECRET_KEY mismatch: canary decrypted but content doesn't match".into(),
        ))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::items_after_statements)]
mod tests {
    use super::*;

    #[test]
    fn manifest_roundtrip() {
        let key = test_key();
        let canary = create_secret_key_canary(&key).unwrap();
        let manifest = BackupManifest {
            format_version: 2,
            schema_version: SCHEMA_VERSION,
            exported_at: OffsetDateTime::now_utc(),
            table_counts: BTreeMap::new(),
            app_version: "0.1.0".into(),
            workspace_file_count: 0,
            workspace_total_bytes: 0,
            secret_key_canary: canary,
            pg_version: "17".into(),
            schemas: vec![
                "config".into(),
                "org".into(),
                "activity".into(),
                "metrics".into(),
                "auth".into(),
                "reasoning".into(),
            ],
            exclude_workspaces: false,
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: BackupManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.format_version, 2);
        assert_eq!(parsed.schema_version, SCHEMA_VERSION);
        assert_eq!(parsed.schemas.len(), 6);
    }

    #[test]
    fn schema_version_check() {
        assert!(check_schema_version(SCHEMA_VERSION).is_ok());
        assert!(check_schema_version(SCHEMA_VERSION - 1).is_err());
        assert!(check_schema_version(SCHEMA_VERSION + 1).is_err());
    }

    #[test]
    fn canary_roundtrip() {
        let key = test_key();
        let canary = create_secret_key_canary(&key).unwrap();
        assert!(validate_secret_key_canary(&canary, &key).is_ok());
    }

    #[test]
    fn canary_wrong_key_fails() {
        let key1 = test_key();
        let key2 = {
            let mut k = [0u8; 32];
            rand::fill(&mut k);
            k
        };
        let canary = create_secret_key_canary(&key1).unwrap();
        assert!(validate_secret_key_canary(&canary, &key2).is_err());
    }

    fn test_key() -> [u8; 32] {
        let mut key = [0u8; 32];
        rand::fill(&mut key);
        key
    }
}
