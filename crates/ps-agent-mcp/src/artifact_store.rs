use bytes::Bytes;
use object_store::ObjectStore;
use object_store::aws::AmazonS3Builder;
use object_store::path::Path;
use std::sync::Arc;

/// Manages uploading and listing conversation artifacts in S3/RustFS.
#[derive(Clone)]
pub struct ArtifactStore {
    store: Arc<dyn ObjectStore>,
    session_id: String,
}

impl ArtifactStore {
    /// Create a new artifact store.
    ///
    /// When `endpoint` is `None`, uses the default AWS S3 endpoint.
    pub fn new(endpoint: Option<&str>, bucket: &str, session_id: &str) -> Self {
        let mut builder = AmazonS3Builder::new()
            .with_bucket_name(bucket)
            .with_allow_http(true);

        if let Some(ep) = endpoint {
            builder = builder.with_endpoint(ep);
        }

        // Use virtual-hosted-style=false for RustFS/MinIO compatibility.
        builder = builder.with_virtual_hosted_style_request(false);

        #[allow(clippy::expect_used)]
        let store = builder.build().expect("failed to build S3 client");

        Self {
            store: Arc::new(store),
            session_id: session_id.to_string(),
        }
    }

    /// Upload a file to S3 under `conversations/{session_id}/{filename}`.
    /// Returns the S3 object key.
    pub async fn upload(
        &self,
        filename: &str,
        content_type: Option<&str>,
        data: Bytes,
    ) -> Result<String, object_store::Error> {
        let key = format!("conversations/{}/{}", self.session_id, filename);
        let path = Path::from(key.as_str());

        let mut opts = object_store::PutOptions::default();
        if let Some(ct) = content_type {
            opts.attributes
                .insert(object_store::Attribute::ContentType, ct.to_string().into());
        }

        self.store.put_opts(&path, data.into(), opts).await?;
        Ok(key)
    }

    /// List all artifacts for the current session.
    pub async fn list(&self) -> Result<Vec<ArtifactEntry>, object_store::Error> {
        use futures::TryStreamExt;

        let prefix = Path::from(format!("conversations/{}/", self.session_id));
        let entries: Vec<_> = self.store.list(Some(&prefix)).try_collect().await?;

        Ok(entries
            .into_iter()
            .map(|meta| {
                let key = meta.location.to_string();
                let filename = key.rsplit('/').next().unwrap_or(&key).to_string();
                ArtifactEntry {
                    key,
                    filename,
                    size_bytes: meta.size.cast_signed(),
                }
            })
            .collect())
    }
}

/// A listed artifact entry.
pub struct ArtifactEntry {
    pub key: String,
    pub filename: String,
    pub size_bytes: i64,
}
