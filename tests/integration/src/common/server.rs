use std::net::SocketAddr;
use std::sync::Arc;

use ps_proto::canonical::prism::v1::admin_service_server::AdminServiceServer;
use ps_proto::canonical::prism::v1::auth_service_server::AuthServiceServer;
use ps_proto::canonical::prism::v1::config_service_server::ConfigServiceServer;
use ps_proto::canonical::prism::v1::handlers_service_server::HandlersServiceServer;
use ps_proto::canonical::prism::v1::metrics_service_server::MetricsServiceServer;
use ps_proto::canonical::prism::v1::org_service_server::OrgServiceServer;
use ps_proto::canonical::prism::v1::reasoning_service_server::ReasoningServiceServer;
use ps_reasoning::routing::TaskRouter;
use ps_reasoning::types::AiConfig;
use ps_server::interceptor::AuthLayer;
use ps_server::services::admin::AdminServiceImpl;
use ps_server::services::auth::AuthServiceImpl;
use ps_server::services::config::ConfigServiceImpl;
use ps_server::services::handlers::HandlersServiceImpl;
use ps_server::services::metrics::MetricsServiceImpl;
use ps_server::services::org::OrgServiceImpl;
use ps_server::services::reasoning::ReasoningServiceImpl;
use sqlx::PgPool;
use tokio::sync::RwLock;
use tonic::transport::{Channel, Server};

/// A running test server with a connected client channel.
pub struct TestServer {
    pub addr: SocketAddr,
    pub channel: Channel,
    pub pool: PgPool,
}

/// A fixed test secret key (32 bytes, only used in tests).
fn test_secret_key() -> zeroize::Zeroizing<[u8; 32]> {
    zeroize::Zeroizing::new(*b"test-secret-key-32-bytes-long!!!")
}

impl TestServer {
    /// Start a gRPC server on a random port with a real PG pool.
    pub async fn start(pool: PgPool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind to random port");
        let addr = listener.local_addr().expect("local addr");

        let repos = ps_core::repo::Repos::new(pool.clone());

        let auth_service = AuthServiceImpl::new(repos.clone());
        let admin_service = AdminServiceImpl::new(repos.clone());
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
            None, // no artifact store in tests
            "http://127.0.0.1:1".into(),
        );

        let server = Server::builder()
            .layer(AuthLayer::new(repos.auth.clone()))
            .add_service(AuthServiceServer::new(auth_service))
            .add_service(AdminServiceServer::new(admin_service))
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
        }
    }
}
