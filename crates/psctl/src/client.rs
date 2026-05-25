use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use http::{Request, Response, Uri};
use hyper_util::client::legacy::Client;
use hyper_util::rt::TokioExecutor;
use ps_proto::canonical::prism::v1::{
    backup_service_client::BackupServiceClient, config_service_client::ConfigServiceClient,
    handlers_service_client::HandlersServiceClient, metrics_service_client::MetricsServiceClient,
    org_service_client::OrgServiceClient, reasoning_service_client::ReasoningServiceClient,
};
use rustls::crypto::aws_lc_rs::default_provider;
use tonic_web::{GrpcWebCall, GrpcWebClientLayer, GrpcWebClientService};
use tower::{Layer, Service};

type HyperBody = tonic::body::Body;
type GrpcWebBody = GrpcWebCall<HyperBody>;
type ResponseBody = GrpcWebCall<hyper::body::Incoming>;

pub struct Clients {
    pub backup: BackupServiceClient<AuthedService>,
    pub config: ConfigServiceClient<AuthedService>,
    pub handlers: HandlersServiceClient<AuthedService>,
    pub metrics: MetricsServiceClient<AuthedService>,
    pub org: OrgServiceClient<AuthedService>,
    pub reasoning: ReasoningServiceClient<AuthedService>,
}

type Connector = hyper_rustls::HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>;
type InnerService = GrpcWebClientService<Client<Connector, GrpcWebBody>>;
pub type AuthedService = AuthService<InnerService>;

pub fn connect(server_url: &str, token: Option<&String>) -> anyhow::Result<Clients> {
    let _ = default_provider().install_default();

    let origin: Uri = server_url.parse()?;

    let mut root_store = rustls::RootCertStore::empty();
    let certs = rustls_native_certs::load_native_certs();
    for cert in &certs.certs {
        root_store.add(cert.clone())?;
    }
    if !certs.errors.is_empty() {
        for e in &certs.errors {
            tracing::warn!(error = %e, "error loading native certificate");
        }
    }

    let config = rustls::ClientConfig::builder()
        .with_root_certificates(root_store)
        .with_no_client_auth();

    let https_connector: Connector = hyper_rustls::HttpsConnectorBuilder::new()
        .with_tls_config(config)
        .https_or_http()
        .enable_http1()
        .build();

    let hyper_client: Client<Connector, GrpcWebBody> =
        Client::builder(TokioExecutor::new()).build(https_connector);

    let grpc_web = GrpcWebClientLayer::new().layer(hyper_client);

    let token = token.cloned();
    Ok(Clients {
        backup: BackupServiceClient::with_origin(
            make_authed(grpc_web.clone(), token.as_ref()),
            origin.clone(),
        ),
        config: ConfigServiceClient::with_origin(
            make_authed(grpc_web.clone(), token.as_ref()),
            origin.clone(),
        ),
        handlers: HandlersServiceClient::with_origin(
            make_authed(grpc_web.clone(), token.as_ref()),
            origin.clone(),
        ),
        metrics: MetricsServiceClient::with_origin(
            make_authed(grpc_web.clone(), token.as_ref()),
            origin.clone(),
        ),
        org: OrgServiceClient::with_origin(
            make_authed(grpc_web.clone(), token.as_ref()),
            origin.clone(),
        ),
        reasoning: ReasoningServiceClient::with_origin(
            make_authed(grpc_web, token.as_ref()),
            origin,
        ),
    })
}

fn make_authed(inner: InnerService, token: Option<&String>) -> AuthedService {
    AuthService {
        inner,
        token: token.cloned(),
    }
}

// ---------------------------------------------------------------------------
// Auth service — injects Bearer token into every request
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct AuthService<S> {
    inner: S,
    token: Option<String>,
}

impl<S, ReqBody> Service<Request<ReqBody>> for AuthService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResponseBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        if let Some(ref token) = self.token {
            let bearer = format!("Bearer {token}");
            if let Ok(val) = http::HeaderValue::from_str(&bearer) {
                req.headers_mut().insert("authorization", val);
            }
        }

        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);

        Box::pin(async move { inner.call(req).await })
    }
}
