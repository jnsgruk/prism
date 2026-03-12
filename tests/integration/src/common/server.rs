use std::net::SocketAddr;

use ps_proto::prism::v1::admin_service_server::AdminServiceServer;
use ps_proto::prism::v1::auth_service_server::AuthServiceServer;
use ps_proto::prism::v1::config_service_server::ConfigServiceServer;
use ps_proto::prism::v1::org_service_server::OrgServiceServer;
use ps_server::interceptor;
use ps_server::services::admin::AdminServiceImpl;
use ps_server::services::auth::AuthServiceImpl;
use ps_server::services::config::ConfigServiceImpl;
use ps_server::services::org::OrgServiceImpl;
use sqlx::PgPool;
use tonic::transport::{Channel, Server};

/// A running test server with a connected client channel.
pub struct TestServer {
    pub addr: SocketAddr,
    pub channel: Channel,
    pub pool: PgPool,
}

/// A fixed test secret key (32 bytes, only used in tests).
fn test_secret_key() -> [u8; 32] {
    *b"test-secret-key-32-bytes-long!!!"
}

impl TestServer {
    /// Start a gRPC server on a random port with a real PG pool.
    pub async fn start(pool: PgPool) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind to random port");
        let addr = listener.local_addr().expect("local addr");

        let auth_service = AuthServiceImpl::new(pool.clone());
        let admin_service = AdminServiceImpl::new(pool.clone());
        let org_service = OrgServiceImpl::new(pool.clone());
        let config_service = ConfigServiceImpl::new(pool.clone(), test_secret_key());

        let auth_pool = pool.clone();

        let auth_layer = tower::ServiceBuilder::new()
            .layer(AuthLayer { pool: auth_pool })
            .into_inner();

        let server = Server::builder()
            .layer(auth_layer)
            .add_service(AuthServiceServer::new(auth_service))
            .add_service(AdminServiceServer::new(admin_service))
            .add_service(OrgServiceServer::new(org_service))
            .add_service(ConfigServiceServer::new(config_service));

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

/// Tower layer that runs the async auth interceptor.
#[derive(Clone)]
struct AuthLayer {
    pool: PgPool,
}

impl<S> tower::Layer<S> for AuthLayer {
    type Service = AuthMiddleware<S>;

    fn layer(&self, inner: S) -> Self::Service {
        AuthMiddleware {
            inner,
            pool: self.pool.clone(),
        }
    }
}

#[derive(Clone)]
struct AuthMiddleware<S> {
    inner: S,
    pool: PgPool,
}

impl<S, B> tower::Service<http::Request<B>> for AuthMiddleware<S>
where
    S: tower::Service<http::Request<B>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    B: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Self::Response, Self::Error>> + Send>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: http::Request<B>) -> Self::Future {
        let pool = self.pool.clone();
        let mut svc = self.inner.clone();
        // Swap to ensure readiness is for the cloned service
        std::mem::swap(&mut self.inner, &mut svc);

        Box::pin(async move {
            let method = req.uri().path().to_string();

            if let Some(auth) = req.headers().get("authorization") {
                let dummy = http::Request::builder()
                    .method(http::Method::POST)
                    .uri(req.uri().clone())
                    .header("authorization", auth.clone())
                    .body(())
                    .expect("build dummy request");

                let tonic_req = tonic::Request::from_http(dummy);

                if let Ok(Some(ctx)) =
                    interceptor::validate_request(&pool, &tonic_req, &method).await
                {
                    req.extensions_mut().insert(ctx);
                }
            }

            svc.call(req).await
        })
    }
}
