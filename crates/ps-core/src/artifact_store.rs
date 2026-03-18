use std::fmt;
use std::time::Duration;

use bytes::Bytes;
use futures::TryStreamExt;
use object_store::ObjectStore;
use object_store::path::Path;
use serde::{Deserialize, Serialize};

use crate::Error;

// ---------------------------------------------------------------------------
// ArtifactCategory + ArtifactKey
// ---------------------------------------------------------------------------

/// Category of artifact stored in object storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactCategory {
    /// Generated insight reports, periodic summaries.
    Insights,
    /// Repo scan raw output, logs, analysis artifacts.
    Scans,
    /// Exported conversation transcripts.
    Conversations,
    /// Raw API response cache (debugging/replay).
    Cache,
}

impl ArtifactCategory {
    fn prefix(self) -> &'static str {
        match self {
            Self::Insights => "insights",
            Self::Scans => "scans",
            Self::Conversations => "conversations",
            Self::Cache => "cache",
        }
    }
}

impl fmt::Display for ArtifactCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.prefix())
    }
}

/// Typed wrapper for artifact keys, encoding category + path in the S3 key.
///
/// Renders to `"{category}/{path}"` — e.g. `"insights/2026-03/team-x-report.pdf"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactKey {
    pub category: ArtifactCategory,
    pub path: String,
}

impl ArtifactKey {
    pub fn new(category: ArtifactCategory, path: impl Into<String>) -> Self {
        Self {
            category,
            path: path.into(),
        }
    }

    /// Convert to an `object_store::path::Path`.
    fn to_object_path(&self) -> Path {
        Path::from(format!("{}/{}", self.category.prefix(), self.path))
    }
}

impl fmt::Display for ArtifactKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.category.prefix(), self.path)
    }
}

// ---------------------------------------------------------------------------
// ArtifactStore trait
// ---------------------------------------------------------------------------

/// Application-level wrapper around `object_store::ObjectStore`.
///
/// All services that read/write artifacts go through this trait.
#[async_trait::async_trait]
pub trait ArtifactStore: Send + Sync {
    /// Write bytes to an artifact key.
    async fn put(&self, key: &ArtifactKey, bytes: Bytes) -> Result<(), Error>;

    /// Read bytes from an artifact key.
    async fn get(&self, key: &ArtifactKey) -> Result<Bytes, Error>;

    /// Generate a pre-signed GET URL for direct download.
    async fn presign_get(&self, key: &ArtifactKey, expiry: Duration) -> Result<url::Url, Error>;

    /// Delete an artifact.
    async fn delete(&self, key: &ArtifactKey) -> Result<(), Error>;

    /// List artifact keys under a prefix.
    async fn list(&self, prefix: &str) -> Result<Vec<String>, Error>;

    /// Check if the store is reachable (health check).
    async fn health_check(&self) -> Result<(), Error>;
}

// ---------------------------------------------------------------------------
// S3ArtifactStore — production implementation
// ---------------------------------------------------------------------------

/// S3-compatible artifact store wrapping `object_store::aws::AmazonS3`.
pub struct S3ArtifactStore {
    store: object_store::aws::AmazonS3,
}

impl S3ArtifactStore {
    /// Create from explicit configuration. Typically used in production.
    pub fn new(
        endpoint: &str,
        bucket: &str,
        access_key_id: &str,
        secret_access_key: &str,
        region: &str,
    ) -> Result<Self, Error> {
        let store = object_store::aws::AmazonS3Builder::new()
            .with_endpoint(endpoint)
            .with_bucket_name(bucket)
            .with_access_key_id(access_key_id)
            .with_secret_access_key(secret_access_key)
            .with_region(region)
            .with_allow_http(true) // Dev: non-TLS endpoints
            .build()
            .map_err(|e| Error::Internal(format!("failed to build S3 client: {e}")))?;

        Ok(Self { store })
    }
}

#[async_trait::async_trait]
impl ArtifactStore for S3ArtifactStore {
    async fn put(&self, key: &ArtifactKey, bytes: Bytes) -> Result<(), Error> {
        let payload = object_store::PutPayload::from_bytes(bytes);
        self.store
            .put(&key.to_object_path(), payload)
            .await
            .map_err(|e| Error::Internal(format!("S3 put failed: {e}")))?;
        Ok(())
    }

    async fn get(&self, key: &ArtifactKey) -> Result<Bytes, Error> {
        let result = self
            .store
            .get(&key.to_object_path())
            .await
            .map_err(|e| Error::Internal(format!("S3 get failed: {e}")))?;
        result
            .bytes()
            .await
            .map_err(|e| Error::Internal(format!("S3 read failed: {e}")))
    }

    async fn presign_get(&self, key: &ArtifactKey, expiry: Duration) -> Result<url::Url, Error> {
        use object_store::signer::Signer;
        let signed = self
            .store
            .signed_url(http::Method::GET, &key.to_object_path(), expiry)
            .await
            .map_err(|e| Error::Internal(format!("S3 presign failed: {e}")))?;
        Ok(signed)
    }

    async fn delete(&self, key: &ArtifactKey) -> Result<(), Error> {
        self.store
            .delete(&key.to_object_path())
            .await
            .map_err(|e| Error::Internal(format!("S3 delete failed: {e}")))?;
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>, Error> {
        let path = Path::from(prefix);
        let entries: Vec<object_store::ObjectMeta> = self
            .store
            .list(Some(&path))
            .try_collect()
            .await
            .map_err(|e| Error::Internal(format!("S3 list failed: {e}")))?;
        Ok(entries
            .into_iter()
            .map(|e| e.location.to_string())
            .collect())
    }

    async fn health_check(&self) -> Result<(), Error> {
        // Try to HEAD the root — success means the bucket is reachable.
        let _ = self
            .store
            .list_with_delimiter(Some(&Path::from("")))
            .await
            .map_err(|e| Error::Internal(format!("S3 health check failed: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// LocalArtifactStore — test implementation
// ---------------------------------------------------------------------------

/// Local filesystem artifact store for testing.
///
/// Wraps `object_store::local::LocalFileSystem`.
pub struct LocalArtifactStore {
    store: object_store::local::LocalFileSystem,
}

impl LocalArtifactStore {
    /// Create a store backed by a local directory.
    pub fn new(root: &std::path::Path) -> Result<Self, Error> {
        let store = object_store::local::LocalFileSystem::new_with_prefix(root)
            .map_err(|e| Error::Internal(format!("failed to create local store: {e}")))?;
        Ok(Self { store })
    }
}

#[async_trait::async_trait]
impl ArtifactStore for LocalArtifactStore {
    async fn put(&self, key: &ArtifactKey, bytes: Bytes) -> Result<(), Error> {
        let payload = object_store::PutPayload::from_bytes(bytes);
        self.store
            .put(&key.to_object_path(), payload)
            .await
            .map_err(|e| Error::Internal(format!("local put failed: {e}")))?;
        Ok(())
    }

    async fn get(&self, key: &ArtifactKey) -> Result<Bytes, Error> {
        let result = self
            .store
            .get(&key.to_object_path())
            .await
            .map_err(|e| Error::Internal(format!("local get failed: {e}")))?;
        result
            .bytes()
            .await
            .map_err(|e| Error::Internal(format!("local read failed: {e}")))
    }

    async fn presign_get(&self, _key: &ArtifactKey, _expiry: Duration) -> Result<url::Url, Error> {
        // Local filesystem doesn't support pre-signed URLs.
        Err(Error::Internal(
            "pre-signed URLs not supported for local storage".into(),
        ))
    }

    async fn delete(&self, key: &ArtifactKey) -> Result<(), Error> {
        self.store
            .delete(&key.to_object_path())
            .await
            .map_err(|e| Error::Internal(format!("local delete failed: {e}")))?;
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>, Error> {
        let path = Path::from(prefix);
        let entries: Vec<object_store::ObjectMeta> = self
            .store
            .list(Some(&path))
            .try_collect()
            .await
            .map_err(|e| Error::Internal(format!("local list failed: {e}")))?;
        Ok(entries
            .into_iter()
            .map(|e| e.location.to_string())
            .collect())
    }

    async fn health_check(&self) -> Result<(), Error> {
        Ok(()) // Local filesystem is always "healthy"
    }
}
