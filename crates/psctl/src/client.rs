use tonic::transport::Channel;

#[derive(Clone)]
pub struct AuthInterceptor {
    token: Option<String>,
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

pub fn connect(
    server_url: &str,
    token: Option<&String>,
) -> anyhow::Result<(Channel, AuthInterceptor)> {
    let endpoint: tonic::transport::Endpoint = server_url.parse()?;
    let channel = endpoint.connect_lazy();
    let auth = AuthInterceptor {
        token: token.cloned(),
    };
    Ok((channel, auth))
}
