use ps_proto::prism::v1::{
    admin_service_client::AdminServiceClient, auth_service_client::AuthServiceClient,
    config_service_client::ConfigServiceClient, handlers_service_client::HandlersServiceClient,
    metrics_service_client::MetricsServiceClient, org_service_client::OrgServiceClient,
};
use tonic::transport::Channel;

#[derive(Clone)]
pub struct AuthInterceptor {
    token: Option<String>,
}

type AuthedChannel = tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>;

/// Pre-constructed gRPC clients for all services, with auth interceptor.
pub struct Clients {
    pub admin: AdminServiceClient<AuthedChannel>,
    pub auth: AuthServiceClient<AuthedChannel>,
    pub config: ConfigServiceClient<AuthedChannel>,
    pub handlers: HandlersServiceClient<AuthedChannel>,
    pub metrics: MetricsServiceClient<AuthedChannel>,
    pub org: OrgServiceClient<AuthedChannel>,
}

impl tonic::service::Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(ref token) = self.token {
            let bearer = format!("Bearer {token}");
            req.metadata_mut().insert(
                "authorization",
                bearer
                    .parse()
                    .map_err(|_| tonic::Status::internal("invalid auth token"))?,
            );
        }
        Ok(req)
    }
}

pub fn connect(server_url: &str, token: Option<&String>) -> anyhow::Result<Clients> {
    let endpoint: tonic::transport::Endpoint = server_url.parse()?;
    let channel = endpoint.connect_lazy();
    let auth = AuthInterceptor {
        token: token.cloned(),
    };
    Ok(Clients {
        admin: AdminServiceClient::with_interceptor(channel.clone(), auth.clone()),
        auth: AuthServiceClient::with_interceptor(channel.clone(), auth.clone()),
        config: ConfigServiceClient::with_interceptor(channel.clone(), auth.clone()),
        handlers: HandlersServiceClient::with_interceptor(channel.clone(), auth.clone()),
        metrics: MetricsServiceClient::with_interceptor(channel.clone(), auth.clone()),
        org: OrgServiceClient::with_interceptor(channel, auth),
    })
}
