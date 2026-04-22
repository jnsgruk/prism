//! Backup/restore archive format for `.ps-backup` files.
//!
//! V2 archives contain a JSON manifest, a `pg_dump --format=custom` database
//! dump, and optional workspace files in a gzipped tar.
mod manifest;

pub use manifest::{
    BackupManifest, CHECKSUM_ENTRY_NAME, SCHEMA_VERSION, check_schema_version,
    create_secret_key_canary, validate_secret_key_canary,
};
