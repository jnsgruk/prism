use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use ps_core::backup::{BackupManifest, create_secret_key_canary};
use ps_proto::canonical::prism::v1::admin_service_server::AdminServiceServer;
use ps_proto::canonical::prism::v1::auth_service_server::AuthServiceServer;
use ps_proto::canonical::prism::v1::backup_service_server::BackupServiceServer;
use ps_proto::canonical::prism::v1::config_service_server::ConfigServiceServer;
use ps_proto::canonical::prism::v1::handlers_service_server::HandlersServiceServer;
use ps_proto::canonical::prism::v1::metrics_service_server::MetricsServiceServer;
use ps_proto::canonical::prism::v1::org_service_server::OrgServiceServer;
use ps_proto::canonical::prism::v1::reasoning_service_server::ReasoningServiceServer;
use ps_reasoning::routing::TaskRouter;
use ps_reasoning::types::AiConfig;
use ps_server::features::admin::AdminServiceImpl;
use ps_server::features::auth::AuthServiceImpl;
use ps_server::features::backup::{BackupGenerator, BackupJobStatus, BackupServiceImpl};
use ps_server::features::config::ConfigServiceImpl;
use ps_server::features::dispatch::HandlersServiceImpl;
use ps_server::features::metrics::MetricsServiceImpl;
use ps_server::features::org::OrgServiceImpl;
use ps_server::features::reasoning::ReasoningServiceImpl;
use ps_server::interceptor::AuthLayer;
use sqlx::PgPool;
use tokio::sync::{Notify, RwLock};
use tonic::transport::{Channel, Server};
use zeroize::Zeroizing;

/// A running test server with a connected client channel.
pub struct TestServer {
    pub addr: SocketAddr,
    pub channel: Channel,
    pub pool: PgPool,
    _backups_dir: tempfile::TempDir,
    /// Signalled when the backup generator starts a backup.
    /// Tests can await this to know the export has started.
    pub backup_started: Arc<Notify>,
}

/// Test context for API-layer tests with a real gRPC server and PostgreSQL.
pub struct ApiTestContext {
    pub server: TestServer,
    db: super::db::TestDb,
}

impl ApiTestContext {
    pub async fn new() -> Self {
        let db = super::db::TestDb::new().await;
        let server = TestServer::start(db.pool.clone()).await;
        Self { server, db }
    }

    pub async fn teardown(self) {
        self.db.teardown().await;
    }
}

/// A fixed test secret key (32 bytes, only used in tests).
fn test_secret_key() -> Zeroizing<[u8; 32]> {
    Zeroizing::new(*b"test-secret-key-32-bytes-long!!!")
}

/// Database schemas listed in manifests (matches ps-backup crate).
const SCHEMAS: &[&str] = &["config", "org", "activity", "metrics", "auth", "reasoning"];

/// Stub implementation of [`BackupGenerator`] for integration tests.
///
/// Backup creates a valid `.ps-backup` archive with a real manifest and a
/// dummy `database.dump` (a few zero bytes). Restore is a no-op — the test
/// database is not wiped or reloaded, so any data seeded before the test
/// remains in place.
///
/// This tests our orchestration (gRPC streaming, manifest validation, canary
/// checks, Job polling, session creation) without depending on `pg_dump` or
/// `pg_restore` being installed on the host.
struct StubBackupGenerator {
    backups_path: PathBuf,
    secret_key: Zeroizing<[u8; 32]>,
    status: Arc<RwLock<Option<(String, BackupJobStatus)>>>,
    started_notify: Arc<Notify>,
}

impl StubBackupGenerator {
    fn new(
        backups_path: PathBuf,
        secret_key: Zeroizing<[u8; 32]>,
        started_notify: Arc<Notify>,
    ) -> Self {
        Self {
            backups_path,
            secret_key,
            status: Arc::new(RwLock::new(None)),
            started_notify,
        }
    }
}

#[async_trait::async_trait]
impl BackupGenerator for StubBackupGenerator {
    async fn start_backup(
        &self,
        backup_id: &str,
        exclude_workspaces: bool,
    ) -> Result<(), tonic::Status> {
        let backup_id = backup_id.to_owned();
        let backups_path = self.backups_path.clone();
        let secret_key = self.secret_key.clone();
        let status = self.status.clone();
        let started_notify = self.started_notify.clone();
        let _ = exclude_workspaces;

        tokio::spawn(async move {
            started_notify.notify_one();

            match build_stub_archive(&backups_path, &backup_id, &secret_key) {
                Ok(()) => {
                    *status.write().await = Some((backup_id, BackupJobStatus::Succeeded));
                }
                Err(e) => {
                    *status.write().await = Some((backup_id, BackupJobStatus::Failed(e)));
                }
            }
        });

        Ok(())
    }

    async fn start_restore(&self, backup_id: &str) -> Result<(), tonic::Status> {
        let backup_id = backup_id.to_owned();
        let backups_path = self.backups_path.clone();
        let status = self.status.clone();

        tokio::spawn(async move {
            // Clean up the staged archive (mirrors real restore behaviour)
            let archive_path = backups_path.join(format!("{backup_id}.ps-backup"));
            let _ = std::fs::remove_file(&archive_path);

            // No-op on the database — data is already in the test DB
            *status.write().await = Some((backup_id, BackupJobStatus::Succeeded));
        });

        Ok(())
    }

    async fn cancel_backup(&self) -> Result<bool, tonic::Status> {
        // Stub completes instantly — cancellation is always a no-op.
        Ok(false)
    }

    async fn poll_status(&self, backup_id: &str) -> Result<BackupJobStatus, tonic::Status> {
        let guard = self.status.read().await;
        match guard.as_ref() {
            Some((id, BackupJobStatus::Succeeded)) if id == backup_id => {
                Ok(BackupJobStatus::Succeeded)
            }
            Some((id, BackupJobStatus::Failed(msg))) if id == backup_id => {
                Ok(BackupJobStatus::Failed(msg.clone()))
            }
            _ => Ok(BackupJobStatus::Running),
        }
    }

    async fn is_backup_active(&self) -> Result<bool, tonic::Status> {
        Ok(false)
    }

    async fn force_cancel(&self) -> Result<(), tonic::Status> {
        Ok(())
    }
}

/// Build a valid `.ps-backup` archive with a real manifest and a dummy dump.
fn build_stub_archive(
    backups_path: &std::path::Path,
    backup_id: &str,
    secret_key: &[u8; 32],
) -> Result<(), String> {
    let canary = create_secret_key_canary(secret_key).map_err(|e| e.to_string())?;

    let manifest = BackupManifest {
        format_version: 2,
        schema_version: 1,
        exported_at: time::OffsetDateTime::now_utc(),
        table_counts: BTreeMap::new(),
        app_version: "0.1.0-test".into(),
        workspace_file_count: 0,
        workspace_total_bytes: 0,
        secret_key_canary: canary,
        pg_version: "17".into(),
        schemas: SCHEMAS.iter().map(|&s| s.to_owned()).collect(),
        exclude_workspaces: true,
    };

    let tmp_path = backups_path.join(format!("{backup_id}.ps-backup.tmp"));
    let final_path = backups_path.join(format!("{backup_id}.ps-backup"));

    let file = std::fs::File::create(&tmp_path).map_err(|e| e.to_string())?;
    let encoder = flate2::write::GzEncoder::new(file, flate2::Compression::default());
    let mut tar = tar::Builder::new(encoder);

    // Write manifest.json
    let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    let mut header = tar::Header::new_gnu();
    header.set_size(manifest_json.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "manifest.json", manifest_json.as_slice())
        .map_err(|e| e.to_string())?;

    // Write a dummy database.dump (restore stub ignores it, but the archive
    // must contain the entry so preview/restore manifest reading succeeds)
    let dummy_dump = b"STUB";
    let mut header = tar::Header::new_gnu();
    header.set_size(dummy_dump.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    tar.append_data(&mut header, "database.dump", dummy_dump.as_slice())
        .map_err(|e| e.to_string())?;

    let encoder = tar.into_inner().map_err(|e| e.to_string())?;
    encoder.finish().map_err(|e| e.to_string())?;

    std::fs::rename(&tmp_path, &final_path).map_err(|e| e.to_string())?;

    Ok(())
}

impl TestServer {
    /// Start a gRPC server on a random port with a real PG pool.
    pub async fn start(pool: PgPool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind to random port");
        let addr = listener.local_addr().expect("local addr");

        let repos = ps_core::repo::Repos::new(pool.clone());
        let backups_dir = tempfile::tempdir().expect("create temp backups dir");
        let backup_started = Arc::new(Notify::new());

        // Stub backup generator — no pg_dump/pg_restore needed
        let generator = Arc::new(StubBackupGenerator::new(
            backups_dir.path().to_path_buf(),
            test_secret_key(),
            backup_started.clone(),
        ));

        let auth_service = AuthServiceImpl::new(repos.clone());
        let admin_service = AdminServiceImpl::new(repos.clone(), None, None);
        let backup_service = BackupServiceImpl::new(
            repos.clone(),
            test_secret_key(),
            Some(backups_dir.path().to_path_buf()),
            generator,
            None, // no post-restore hook needed in tests
        );
        let org_service = OrgServiceImpl::new(repos.clone());
        let config_service = ConfigServiceImpl::new(repos.clone(), test_secret_key());
        let metrics_service = MetricsServiceImpl::new(repos.clone());
        // HandlersService uses a dummy Restate URL — trigger tests will get
        // connection-refused, which is expected (we test the gRPC layer, not Restate).
        let handlers_service = HandlersServiceImpl::new(
            repos.clone(),
            "http://127.0.0.1:1".into(),
            "http://127.0.0.1:1".into(),
        );
        let router = Arc::new(RwLock::new(TaskRouter::new(AiConfig::default())));
        let reasoning_service = ReasoningServiceImpl::new(
            repos.clone(),
            test_secret_key(),
            router,
            None, // no workspaces path in tests
            "http://127.0.0.1:1".into(),
        );

        let server = Server::builder()
            .layer(AuthLayer::new(repos.auth.clone()))
            .add_service(AuthServiceServer::new(auth_service))
            .add_service(AdminServiceServer::new(admin_service))
            .add_service(BackupServiceServer::new(backup_service))
            .add_service(OrgServiceServer::new(org_service))
            .add_service(ConfigServiceServer::new(config_service))
            .add_service(HandlersServiceServer::new(handlers_service))
            .add_service(MetricsServiceServer::new(metrics_service))
            .add_service(ReasoningServiceServer::new(reasoning_service));

        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);

        tokio::spawn(async move {
            server
                .serve_with_incoming(incoming)
                .await
                .expect("server error");
        });

        let channel = Channel::from_shared(format!("http://{addr}"))
            .expect("valid uri")
            .connect()
            .await
            .expect("connect to test server");

        Self {
            addr,
            channel,
            pool,
            _backups_dir: backups_dir,
            backup_started,
        }
    }
}
