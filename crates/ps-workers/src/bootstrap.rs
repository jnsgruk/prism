use tracing::{error, info, warn};

pub(super) fn spawn_bootstrap_tasks() {
    let restate_admin_url =
        std::env::var("RESTATE_ADMIN_URL").unwrap_or_else(|_| "http://restate:9070".into());
    let restate_port = std::env::var("PS_RESTATE_LISTEN_PORT").unwrap_or_else(|_| "9081".into());
    let self_url = std::env::var("RESTATE_SELF_URL")
        .unwrap_or_else(|_| format!("http://ps-workers:{restate_port}"));
    let restate_ingress_url =
        std::env::var("RESTATE_URL").unwrap_or_else(|_| "http://restate:8080".into());

    tokio::spawn(async move {
        register_with_restate(&restate_admin_url, &self_url).await;
        for (service, handler) in [
            ("AgentPodReaperHandler", "reap"),
            ("QueryWatchdogHandler", "check"),
            ("SnapshotRefreshHandler", "recover"),
        ] {
            bootstrap_loop(&restate_ingress_url, &restate_admin_url, service, handler).await;
        }
    });
}

/// Register this service deployment with the Restate admin API.
///
/// Retries up to 10 times with exponential backoff to handle startup
/// ordering (Restate may not be ready yet when we start).
async fn register_with_restate(admin_url: &str, self_url: &str) {
    let client = reqwest::Client::new();
    let url = format!("{admin_url}/deployments");

    for attempt in 1u64..=10 {
        let body = serde_json::json!({
            "uri": self_url,
            "force": true,
        });

        match client.post(&url).json(&body).send().await {
            Ok(resp) if resp.status().is_success() => {
                info!(self_url, "registered with Restate");
                return;
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                warn!(
                    attempt,
                    %status,
                    body,
                    "failed to register with Restate, retrying"
                );
            }
            Err(e) => {
                warn!(attempt, "cannot reach Restate admin: {e}");
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(attempt * 2)).await;
    }

    error!("gave up registering with Restate after 10 attempts");
}

/// Resume a durable singleton loop only when no scheduled or active invocation
/// already owns it. Restarts must not fork recurring recovery chains.
async fn bootstrap_loop(ingress_url: &str, admin_url: &str, service: &str, handler: &str) {
    let client = reqwest::Client::new();
    let sql = serde_json::json!({
        "query": format!("SELECT COUNT(*) AS cnt FROM sys_invocation \
                  WHERE target_service_name = '{service}' \
                  AND status IN ('scheduled', 'running', 'suspended', 'ready', 'backing-off')")
    });
    match client
        .post(format!("{admin_url}/query"))
        .header("Accept", "application/json")
        .json(&sql)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {
            if let Ok(body) = resp.json::<serde_json::Value>().await {
                let active_count = body
                    .get("rows")
                    .and_then(|rows| rows.as_array())
                    .and_then(|rows| rows.first())
                    .and_then(|row| row.get("cnt"))
                    .and_then(serde_json::Value::as_i64)
                    .unwrap_or(0);
                if active_count > 0 {
                    info!(service, active_count, "recovery loop already active");
                    return;
                }
            }
        }
        Ok(resp) => warn!(service, status = %resp.status(), "could not check recovery invocations"),
        Err(error) => warn!(service, %error, "could not reach Restate admin"),
    }

    let url = format!("{ingress_url}/{service}/singleton/{handler}/send");
    match client.post(url).send().await {
        Ok(resp) if resp.status().is_success() => info!(service, "bootstrapped recovery loop"),
        Ok(resp) => error!(service, status = %resp.status(), "failed to bootstrap recovery loop"),
        Err(error) => error!(service, %error, "failed to send recovery bootstrap"),
    }
}
