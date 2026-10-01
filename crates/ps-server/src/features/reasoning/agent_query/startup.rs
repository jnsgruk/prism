//! Bounded, read-only retries and reconciliation of ambiguous session creation.

use std::time::Duration;

use ps_agent::opencode_sdk::types::session::Session;
use ps_agent::readiness::{POLL_INTERVAL, startup_http_client};
use tokio::time::{Instant, timeout_at};

pub type StartupError = Box<dyn std::error::Error + Send + Sync>;
const SESSION_TIMEOUT: Duration = Duration::from_secs(15);

/// Keep startup deadlines separate from normal operation and streaming clients.
pub struct SessionStartup {
    http: reqwest::Client,
    base_url: String,
    deadline: Instant,
    session_timeout: Duration,
}

impl SessionStartup {
    pub fn new(base_url: String, deadline: Instant) -> Result<Self, StartupError> {
        Ok(Self {
            http: startup_http_client()?,
            base_url,
            deadline,
            session_timeout: SESSION_TIMEOUT,
        })
    }

    pub async fn ready(&self) -> Result<(), StartupError> {
        ps_agent::readiness::wait_for_opencode(&self.http, &self.base_url, self.deadline)
            .await
            .map_err(Into::into)
    }

    async fn pause(&self) -> Result<(), StartupError> {
        timeout_at(self.deadline, tokio::time::sleep(POLL_INTERVAL))
            .await
            .map_err(|_| "agent startup budget exhausted")?;
        Ok(())
    }

    /// GET retries are safe. Only a definitive 404 means the session is gone.
    pub async fn get(&self, id: &str) -> Result<Option<Session>, StartupError> {
        self.read(&format!("/session/{id}")).await
    }

    async fn read<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<Option<T>, StartupError> {
        loop {
            if Instant::now() >= self.deadline {
                return Err("agent startup budget exhausted resolving session".into());
            }
            let request = async {
                let response = self
                    .http
                    .get(format!("{}{path}", self.base_url))
                    .header("x-opencode-directory", "/home/agent")
                    .send()
                    .await?;
                if response.status() == reqwest::StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                response.error_for_status()?.json().await.map(Some)
            };
            match timeout_at(self.deadline, request).await {
                Ok(Ok(value)) => return Ok(value),
                Ok(Err(e)) if e.status().is_some_and(|s| s.is_client_error()) => {
                    return Err(e.into());
                }
                Ok(Err(e)) => {
                    tracing::warn!(error = ?e, path, "Transient OpenCode startup read failure");
                }
                Err(_) => return Err("agent startup budget exhausted resolving session".into()),
            }
            self.pause().await?;
        }
    }

    pub async fn find(&self, title: &str) -> Result<Option<Session>, StartupError> {
        let sessions: Vec<Session> = self
            .read("/session")
            .await?
            .ok_or("OpenCode session list endpoint unavailable")?;
        Ok(sessions
            .into_iter()
            .find(|s| s.title == title && s.parent_id.is_none()))
    }

    /// Never retry POST: even a timeout, broken response body, or server error
    /// may mean the session was committed. Poll the stable title instead.
    pub async fn create_or_reconcile(
        &self,
        title: &str,
        create: bool,
    ) -> Result<Session, StartupError> {
        if Instant::now() >= self.deadline {
            return Err("agent startup budget exhausted resolving session".into());
        }
        if create {
            let started = Instant::now();
            let request = async {
                self.http
                    .post(format!("{}/session", self.base_url))
                    .timeout(self.session_timeout)
                    .header("x-opencode-directory", "/home/agent")
                    .json(&serde_json::json!({"title": title}))
                    .send()
                    .await?
                    .error_for_status()?
                    .json::<Session>()
                    .await
            };
            match timeout_at(self.deadline, request).await {
                Ok(Ok(session)) => return Ok(session),
                result => {
                    tracing::warn!(elapsed_ms = started.elapsed().as_millis(), result = ?result,
                    "Ambiguous OpenCode session creation; reconciling without another POST");
                }
            }
        }
        loop {
            if let Some(session) = self.find(title).await? {
                return Ok(session);
            }
            self.pause().await?;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn startup(server: &MockServer) -> SessionStartup {
        SessionStartup::new(server.uri(), Instant::now() + Duration::from_secs(2)).unwrap()
    }

    /// Run only against an isolated diagnostic pod, never a user's agent.
    #[tokio::test]
    #[ignore = "requires PRISM_STARTUP_PROBE_URL pointing at an isolated OpenCode pod"]
    async fn live_readiness_create_reuse_and_sse() {
        let _ = tracing_subscriber::fmt().with_test_writer().try_init();
        let base_url = std::env::var("PRISM_STARTUP_PROBE_URL").unwrap();
        let started = Instant::now();
        let startup =
            SessionStartup::new(base_url.clone(), started + Duration::from_secs(30)).unwrap();
        startup.ready().await.unwrap();
        let title = format!("Prism diagnostic {}", uuid::Uuid::new_v4());
        let create_started = Instant::now();
        let session = startup.create_or_reconcile(&title, true).await.unwrap();
        tracing::info!(
            elapsed_ms = create_started.elapsed().as_millis(),
            "Diagnostic session created"
        );
        let reuse_started = Instant::now();
        assert_eq!(
            startup.get(&session.id).await.unwrap().unwrap().id,
            session.id
        );
        tracing::info!(
            elapsed_ms = reuse_started.elapsed().as_millis(),
            "Diagnostic session reused"
        );
        let client = ps_agent::opencode_sdk::ClientBuilder::new()
            .base_url(base_url)
            .directory("/home/agent")
            .timeout_secs(120)
            .build()
            .unwrap();
        let sse_started = Instant::now();
        let subscription = super::super::session::subscribe_to_events(&client)
            .await
            .unwrap();
        tracing::info!(
            elapsed_ms = sse_started.elapsed().as_millis(),
            total_ms = started.elapsed().as_millis(),
            "Diagnostic SSE connected"
        );
        subscription.close();
    }

    #[tokio::test]
    async fn transient_lookup_failure_preserves_existing_session() {
        let server = MockServer::start().await;
        Mock::given(path("/session/ses_existing"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"id": "ses_existing"})),
            )
            .mount(&server)
            .await;
        Mock::given(path("/session/ses_existing"))
            .respond_with(ResponseTemplate::new(503))
            .with_priority(1)
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            startup(&server)
                .get("ses_existing")
                .await
                .unwrap()
                .unwrap()
                .id,
            "ses_existing"
        );
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| r.method == "GET")
        );
    }

    #[tokio::test]
    async fn definitive_not_found_allows_replacement() {
        let server = MockServer::start().await;
        Mock::given(path("/session/ses_expired"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        assert!(startup(&server).get("ses_expired").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn ambiguous_create_reconciles_without_duplicate_post() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/session"))
            .respond_with(ResponseTemplate::new(200).set_body_string("broken response"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": "ses_committed", "title": "stable title"}
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
            .with_priority(1)
            .up_to_n_times(1)
            .mount(&server)
            .await;
        assert_eq!(
            startup(&server)
                .create_or_reconcile("stable title", true)
                .await
                .unwrap()
                .id,
            "ses_committed"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn timed_out_create_recovers_committed_session_without_another_post() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/session"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": "ses_committed"}))
                    .set_delay(Duration::from_millis(200)),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": "ses_committed", "title": "stable title"}
            ])))
            .mount(&server)
            .await;
        let mut startup = startup(&server);
        startup.session_timeout = Duration::from_millis(50);
        assert_eq!(
            startup
                .create_or_reconcile("stable title", true)
                .await
                .unwrap()
                .id,
            "ses_committed"
        );
        server.verify().await;
    }

    #[tokio::test]
    async fn pending_creation_on_follow_up_only_reconciles() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
                {"id": "ses_committed", "title": "stable title"}
            ])))
            .expect(1)
            .mount(&server)
            .await;
        assert_eq!(
            startup(&server)
                .create_or_reconcile("stable title", false)
                .await
                .unwrap()
                .id,
            "ses_committed"
        );
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn lookup_and_reconciliation_exhaust_shared_budget() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(503))
            .mount(&server)
            .await;
        let started = Instant::now();
        let startup =
            SessionStartup::new(server.uri(), started + Duration::from_millis(100)).unwrap();
        assert!(
            startup
                .get("ses_existing")
                .await
                .unwrap_err()
                .to_string()
                .contains("budget exhausted")
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(
            server
                .received_requests()
                .await
                .unwrap()
                .iter()
                .all(|r| r.method == "GET")
        );
    }
}
