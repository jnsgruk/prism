//! Manages ephemeral K8s Pods running `OpenCode` agent containers.
//!
//! Each conversation gets its own Pod. Pods are created on demand, reused for
//! follow-up questions, and reaped when idle or after a maximum lifetime.

use k8s_openapi::api::core::v1::Pod;
use kube::Client as KubeClient;
use kube::api::{Api, DeleteParams, ListParams, PostParams};

use crate::pod_spec::{
    ANNOTATION_LAST_ACTIVITY, ANNOTATION_TOKEN_SESSION_ID, AgentPodConfig, LABEL_APP_VALUE,
    LABEL_SESSION,
};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

/// Size of each workspace PVC.
/// How long a container can be idle before being reaped.
const IDLE_TIMEOUT: Duration = Duration::from_secs(30 * 60); // 30 minutes

/// Maximum container lifetime regardless of activity.
const MAX_LIFETIME: Duration = Duration::from_secs(2 * 60 * 60); // 2 hours

/// Maximum concurrent agent containers.
const MAX_CONTAINERS: usize = 20;

/// Per-pod overrides applied on top of the default `AgentPodConfig`.
#[derive(Clone)]
pub struct PodOverrides {
    pub service_token: String,
    pub token_session_id: String,
    pub model: String,
    pub small_model: String,
    pub provider_keys: Vec<(String, String)>,
    /// Default image generation model, injected as `DEFAULT_IMAGE_MODEL` env var.
    pub default_image_model: Option<String>,
}

/// Information about a pod that was reaped by the idle/expiry reaper.
#[derive(Debug, Clone)]
pub struct ReapedPod {
    /// The token session ID stored as an annotation (for auth session cleanup).
    pub token_session_id: String,
    /// The conversation/session ID from the pod label (for DB status update).
    pub session_id: String,
}

/// Status of an agent Pod.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PodStatus {
    /// Pod is being created or scheduled.
    Pending,
    /// Pod is running and ready.
    Running { pod_ip: String, pod_name: String },
    /// Pod has completed or been deleted.
    Gone,
}

/// Manages the lifecycle of agent container Pods.
#[derive(Clone)]
pub struct ContainerManager {
    kube: KubeClient,
    namespace: String,
    default_config: Arc<AgentPodConfig>,
}

impl ContainerManager {
    /// Create a new `ContainerManager`.
    pub fn new(kube: KubeClient, namespace: String, default_config: AgentPodConfig) -> Self {
        Self {
            kube,
            namespace,
            default_config: Arc::new(default_config),
        }
    }

    /// Find an active Pod for the session, or create a new one.
    ///
    /// Per-pod overrides (model, token, provider keys) are applied on top of
    /// the default config.  `token_session_id` is stored as an annotation so
    /// the background reaper can delete the auth session when the pod is reaped.
    pub async fn ensure_pod(
        &self,
        session_id: &str,
        overrides: &PodOverrides,
    ) -> Result<PodStatus, kube::Error> {
        // Check for existing pod first.
        let status = self.get_pod_status(session_id).await?;
        if let PodStatus::Running { .. } = &status {
            return Ok(status);
        }

        // Check capacity.
        let active = self.count_active_pods().await?;
        if active >= MAX_CONTAINERS {
            return Ok(PodStatus::Gone); // Caller should report capacity error.
        }

        // Build pod with per-session overrides.
        let mut config = (*self.default_config).clone();
        config.service_token = overrides.service_token.clone();
        config.model = overrides.model.clone();
        config.small_model = overrides.small_model.clone();
        if !overrides.provider_keys.is_empty() {
            config.provider_keys = overrides.provider_keys.clone();
        }
        if let Some(ref img_model) = overrides.default_image_model {
            config
                .provider_keys
                .push(("DEFAULT_IMAGE_MODEL".to_string(), img_model.clone()));
        }
        let mut pod = crate::pod_spec::build_agent_pod(session_id, &config);

        // Store the token's session ID so reap_idle_pods can clean it up.
        if let Some(annotations) = pod.metadata.annotations.as_mut() {
            annotations.insert(
                ANNOTATION_TOKEN_SESSION_ID.to_string(),
                overrides.token_session_id.clone(),
            );
        }

        let pods: Api<Pod> = Api::namespaced(self.kube.clone(), &self.namespace);
        let created = pods.create(&PostParams::default(), &pod).await?;

        let pod_name = created
            .metadata
            .name
            .unwrap_or_else(|| "unknown".to_string());
        info!(session_id, pod_name, "Created agent pod");

        Ok(PodStatus::Pending)
    }

    /// Check the status of the Pod for a given session.
    pub async fn get_pod_status(&self, session_id: &str) -> Result<PodStatus, kube::Error> {
        let pods: Api<Pod> = Api::namespaced(self.kube.clone(), &self.namespace);
        let lp = ListParams::default().labels(&format!("{LABEL_SESSION}={session_id}"));

        let list = pods.list(&lp).await?;
        let Some(pod) = list.items.first() else {
            return Ok(PodStatus::Gone);
        };

        let phase = pod
            .status
            .as_ref()
            .and_then(|s| s.phase.as_deref())
            .unwrap_or("Unknown");

        match phase {
            "Running" => {
                let pod_ip = pod
                    .status
                    .as_ref()
                    .and_then(|s| s.pod_ip.clone())
                    .unwrap_or_default();
                let pod_name = pod
                    .metadata
                    .name
                    .clone()
                    .unwrap_or_else(|| "unknown".to_string());
                Ok(PodStatus::Running { pod_ip, pod_name })
            }
            "Pending" => Ok(PodStatus::Pending),
            _ => Ok(PodStatus::Gone),
        }
    }

    /// Poll until the pod for `session_id` reaches the Running phase.
    ///
    /// Returns the pod IP on success, or an error string if the pod
    /// disappears or the 60-second deadline elapses.
    pub async fn wait_for_ready(&self, session_id: &str) -> Result<String, String> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));

        loop {
            interval.tick().await;
            if tokio::time::Instant::now() >= deadline {
                return Err("timed out waiting for agent container".into());
            }

            match self.get_pod_status(session_id).await {
                Ok(PodStatus::Running { pod_ip, .. }) => return Ok(pod_ip),
                Ok(PodStatus::Pending) => {}
                Ok(PodStatus::Gone) => {
                    return Err("agent container failed to start".into());
                }
                Err(e) => {
                    return Err(format!("error checking container status: {e}"));
                }
            }
        }
    }

    /// Update the last-activity annotation on the Pod to prevent reaping.
    pub async fn update_activity(&self, session_id: &str) -> Result<(), kube::Error> {
        let pods: Api<Pod> = Api::namespaced(self.kube.clone(), &self.namespace);
        let lp = ListParams::default().labels(&format!("{LABEL_SESSION}={session_id}"));

        let list = pods.list(&lp).await?;
        if let Some(pod) = list.items.first()
            && let Some(name) = &pod.metadata.name
        {
            let patch = serde_json::json!({
                "metadata": {
                    "annotations": {
                        ANNOTATION_LAST_ACTIVITY: time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default()
                    }
                }
            });
            let patch_params = kube::api::PatchParams::apply("prism-server");
            pods.patch(name, &patch_params, &kube::api::Patch::Merge(&patch))
                .await?;
        }
        Ok(())
    }

    /// Delete the Pod for a session.
    pub async fn delete_pod(&self, session_id: &str) -> Result<(), kube::Error> {
        let pods: Api<Pod> = Api::namespaced(self.kube.clone(), &self.namespace);
        let lp = ListParams::default().labels(&format!("{LABEL_SESSION}={session_id}"));

        let list = pods.list(&lp).await?;
        for pod in &list.items {
            if let Some(name) = &pod.metadata.name {
                match pods.delete(name, &DeleteParams::default()).await {
                    Ok(_) => info!(pod_name = %name, session_id, "Deleted agent pod"),
                    Err(e) => warn!(pod_name = %name, error = %e, "Failed to delete agent pod"),
                }
            }
        }
        Ok(())
    }

    /// Reap idle and expired agent Pods. Intended to run on a timer (every 60s).
    ///
    /// Returns information about each reaped pod so the caller can clean up
    /// auth sessions and update conversation status in the database.
    pub async fn reap_idle_pods(&self) -> Vec<ReapedPod> {
        let pods: Api<Pod> = Api::namespaced(self.kube.clone(), &self.namespace);
        let lp = ListParams::default().labels(&format!("app={LABEL_APP_VALUE}"));

        let list = match pods.list(&lp).await {
            Ok(l) => l,
            Err(e) => {
                error!(error = %e, "Failed to list agent pods for reaping");
                return vec![];
            }
        };

        let now = time::OffsetDateTime::now_utc();
        let mut reaped_pods = Vec::new();

        for pod in &list.items {
            let name = match &pod.metadata.name {
                Some(n) => n.clone(),
                None => continue,
            };

            let mut should_reap = false;

            // Check max lifetime from creation timestamp.
            if let Some(creation) = &pod.metadata.creation_timestamp {
                // k8s_openapi::jiff::Timestamp → string → parse with time crate.
                let ts_str = creation.0.to_string();
                if let Ok(created) = time::OffsetDateTime::parse(
                    &ts_str,
                    &time::format_description::well_known::Rfc3339,
                ) {
                    let age = now - created;
                    if age.unsigned_abs() > MAX_LIFETIME {
                        info!(pod_name = %name, age_secs = age.whole_seconds(), "Reaping expired agent pod");
                        should_reap = true;
                    }
                }
            }

            // Check idle timeout from last-activity annotation.
            if !should_reap {
                let last_activity = pod
                    .metadata
                    .annotations
                    .as_ref()
                    .and_then(|a| a.get(ANNOTATION_LAST_ACTIVITY))
                    .and_then(|ts| {
                        time::OffsetDateTime::parse(
                            ts,
                            &time::format_description::well_known::Rfc3339,
                        )
                        .ok()
                    });

                if let Some(last) = last_activity {
                    let idle = now - last;
                    if idle.unsigned_abs() > IDLE_TIMEOUT {
                        info!(pod_name = %name, idle_secs = idle.whole_seconds(), "Reaping idle agent pod");
                        should_reap = true;
                    }
                }
            }

            if should_reap {
                // Collect token session ID and conversation ID before deleting.
                let token_session_id = pod
                    .metadata
                    .annotations
                    .as_ref()
                    .and_then(|a| a.get(ANNOTATION_TOKEN_SESSION_ID))
                    .cloned()
                    .unwrap_or_default();
                let session_id = pod
                    .metadata
                    .labels
                    .as_ref()
                    .and_then(|l| l.get(LABEL_SESSION))
                    .cloned()
                    .unwrap_or_default();

                if !token_session_id.is_empty() || !session_id.is_empty() {
                    reaped_pods.push(ReapedPod {
                        token_session_id,
                        session_id,
                    });
                }
                let _ = pods.delete(&name, &DeleteParams::default()).await;
            }
        }

        reaped_pods
    }

    /// Count currently active (Pending + Running) agent Pods.
    async fn count_active_pods(&self) -> Result<usize, kube::Error> {
        let pods: Api<Pod> = Api::namespaced(self.kube.clone(), &self.namespace);
        let lp = ListParams::default().labels(&format!("app={LABEL_APP_VALUE}"));
        let list = pods.list(&lp).await?;
        Ok(list.items.len())
    }

    /// Create an `OpenCode` SDK client pointing at the given Pod IP.
    pub fn opencode_client(
        pod_ip: &str,
    ) -> Result<opencode_sdk::Client, opencode_sdk::OpencodeError> {
        opencode_sdk::ClientBuilder::new()
            .base_url(format!("http://{pod_ip}:{}", crate::OPENCODE_PORT))
            .directory("/home/agent")
            .timeout_secs(120)
            .build()
    }
}
