use std::collections::HashMap;
use std::io::{Read, Write};

use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::Error;

/// Metadata about a backup archive, stored as the first entry in the tar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub schema_version: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub exported_at: OffsetDateTime,
    pub table_counts: HashMap<String, i32>,
    pub app_version: String,
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
        let mut data = Vec::new();
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

/// Reads a `.ps-backup` archive, providing access to manifest and table entries.
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

    /// Read the manifest from the archive. Must be the first entry.
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
            schema_version: 5,
            exported_at: OffsetDateTime::now_utc(),
            table_counts: HashMap::from([("users".into(), 3), ("teams".into(), 2)]),
            app_version: "0.1.0".into(),
        };

        let json = serde_json::to_string(&manifest).unwrap();
        let parsed: BackupManifest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.schema_version, 5);
        assert_eq!(parsed.table_counts.len(), 2);
    }

    #[test]
    fn write_and_read_archive() {
        let manifest = BackupManifest {
            schema_version: 5,
            exported_at: OffsetDateTime::now_utc(),
            table_counts: HashMap::from([("test_table".into(), 2)]),
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
        assert_eq!(read_manifest.schema_version, 5);
        assert_eq!(read_manifest.app_version, "0.1.0");
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
}
