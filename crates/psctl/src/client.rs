use ps_proto::prism::v1::{
    admin_service_client::AdminServiceClient, auth_service_client::AuthServiceClient,
    config_service_client::ConfigServiceClient, handlers_service_client::HandlersServiceClient,
};
use tonic::transport::Channel;

#[derive(Clone)]
pub struct AuthInterceptor {
    token: Option<String>,
}

/// Pre-constructed gRPC clients for all services, with auth interceptor.
pub struct Clients {
    pub admin: AdminServiceClient<
        tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
    >,
    pub auth: AuthServiceClient<
        tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
    >,
    pub config: ConfigServiceClient<
        tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
    >,
    pub handlers: HandlersServiceClient<
        tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>,
    >,
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
        handlers: HandlersServiceClient::with_interceptor(channel, auth),
    })
}
