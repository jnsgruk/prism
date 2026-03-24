use ps_proto::canonical::prism::v1::{
    insights_service_client::InsightsServiceClient, metrics_service_client::MetricsServiceClient,
    org_service_client::OrgServiceClient, reasoning_service_client::ReasoningServiceClient,
};
use tonic::transport::Channel;

/// Auth interceptor that attaches a Bearer token to every request.
#[derive(Clone)]
pub struct AuthInterceptor {
    token: String,
}

impl tonic::service::Interceptor for AuthInterceptor {
    fn call(&mut self, mut req: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
        if !self.token.is_empty() {
            let bearer = format!("Bearer {}", self.token);
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

pub type AuthedChannel = tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>;

/// gRPC client bundle for calling ps-server from inside the agent container.
#[derive(Clone)]
pub struct PrismClient {
    pub metrics: MetricsServiceClient<AuthedChannel>,
    pub org: OrgServiceClient<AuthedChannel>,
    pub reasoning: ReasoningServiceClient<AuthedChannel>,
    pub insights: InsightsServiceClient<AuthedChannel>,
}

impl PrismClient {
    /// Create a lazily-connected client bundle.
    pub fn connect(url: &str, token: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let endpoint: tonic::transport::Endpoint = url.parse()?;
        let channel = endpoint.connect_lazy();
        let auth = AuthInterceptor {
            token: token.to_string(),
        };

        Ok(Self {
            metrics: MetricsServiceClient::with_interceptor(channel.clone(), auth.clone()),
            org: OrgServiceClient::with_interceptor(channel.clone(), auth.clone()),
            reasoning: ReasoningServiceClient::with_interceptor(channel.clone(), auth.clone()),
            insights: InsightsServiceClient::with_interceptor(channel, auth),
        })
    }
}
