//! Inexpensive application readiness checks, independent of operation/SSE timeouts.

use std::time::Duration;
use tokio::time::{Instant, timeout_at};

pub const HEALTH_PATH: &str = "/global/health";
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(1);
pub const READINESS_TIMEOUT: Duration = Duration::from_secs(3);
pub const POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Direct pod traffic must not pass through an environment HTTP proxy.
pub fn startup_http_client() -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(CONNECT_TIMEOUT)
        .timeout(READINESS_TIMEOUT)
        .build()
}

/// Verify both the HTTP response and `OpenCode`'s health payload. A global health
/// check does not initialise the directory, MCP servers, or model providers.
pub async fn wait_for_opencode(
    client: &reqwest::Client,
    base_url: &str,
    deadline: Instant,
) -> Result<(), String> {
    let started = Instant::now();
    let mut attempt = 0_u32;
    loop {
        if Instant::now() >= deadline {
            return Err("agent startup budget exhausted waiting for OpenCode readiness".into());
        }
        attempt += 1;
        let attempt_started = Instant::now();
        let check = async {
            let health: serde_json::Value = client
                .get(format!("{base_url}{HEALTH_PATH}"))
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            Ok::<bool, reqwest::Error>(
                health.get("healthy") == Some(&serde_json::Value::Bool(true)),
            )
        };
        let result = timeout_at(deadline, check).await;
        match result {
            Ok(Ok(true)) => {
                tracing::info!(
                    attempt,
                    base_url,
                    elapsed_ms = started.elapsed().as_millis(),
                    "OpenCode application ready"
                );
                return Ok(());
            }
            Ok(result) => {
                tracing::warn!(attempt, attempt_ms = attempt_started.elapsed().as_millis(), result = ?result, "OpenCode readiness check failed");
            }
            Err(_) => {
                return Err("agent startup budget exhausted waiting for OpenCode readiness".into());
            }
        }
        if timeout_at(deadline, tokio::time::sleep(POLL_INTERVAL))
            .await
            .is_err()
        {
            return Err("agent startup budget exhausted waiting for OpenCode readiness".into());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn delayed_readiness_retries_until_healthy() {
        let server = MockServer::start().await;
        Mock::given(path(HEALTH_PATH))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"healthy": true})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(HEALTH_PATH))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(2)
            .expect(2)
            .with_priority(1)
            .mount(&server)
            .await;
        wait_for_opencode(
            &startup_http_client().unwrap(),
            &server.uri(),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
        assert_eq!(server.received_requests().await.unwrap().len(), 3);
    }

    #[tokio::test]
    async fn stalled_readiness_is_bounded_by_overall_budget() {
        let server = MockServer::start().await;
        Mock::given(path(HEALTH_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"healthy": true}))
                    .set_delay(Duration::from_secs(5)),
            )
            .mount(&server)
            .await;
        let started = Instant::now();
        let err = wait_for_opencode(
            &startup_http_client().unwrap(),
            &server.uri(),
            started + Duration::from_millis(100),
        )
        .await
        .unwrap_err();
        assert!(err.contains("budget exhausted"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn transient_connection_failure_recovers_when_server_starts() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let server = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let listener = tokio::net::TcpListener::bind(address).await.unwrap();
            let (mut socket, _) = listener.accept().await.unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut request = [0_u8; 1024];
            socket.read(&mut request).await.unwrap();
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"healthy\":true}").await.unwrap();
        });
        wait_for_opencode(
            &startup_http_client().unwrap(),
            &format!("http://{address}"),
            Instant::now() + Duration::from_secs(2),
        )
        .await
        .unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn connection_failure_consumes_only_the_startup_budget() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let started = Instant::now();
        let err = wait_for_opencode(
            &startup_http_client().unwrap(),
            &format!("http://{address}"),
            started + Duration::from_millis(100),
        )
        .await
        .unwrap_err();
        assert!(err.contains("budget exhausted"));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
