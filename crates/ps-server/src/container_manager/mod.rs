//! Manages ephemeral K8s Pods running `OpenCode` agent containers.
//!
//! Each conversation gets its own Pod. Pods are created on demand, reused for
//! follow-up questions, and reaped when idle or after a maximum lifetime.

pub mod event_mapper;
pub mod pod_spec;

use k8s_openapi::api::core::v1::Pod;
use kube::Client as KubeClient;
use kube::api::{Api, DeleteParams, ListParams, PostParams};
use pod_spec::{ANNOTATION_LAST_ACTIVITY, AgentPodConfig, LABEL_APP_VALUE, LABEL_SESSION};
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

/// Port exposed by the `OpenCode` server inside each agent container.
pub const OPENCODE_PORT: u16 = 4096;

/// How long a container can be idle before being reaped.
const IDLE_TIMEOUT: Duration = Duration::from_secs(15 * 60); // 15 minutes

/// Maximum container lifetime regardless of activity.
const MAX_LIFETIME: Duration = Duration::from_secs(2 * 60 * 60); // 2 hours

/// Maximum concurrent agent containers.
const MAX_CONTAINERS: usize = 20;

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
    /// Returns the Pod IP and name when ready.
    pub async fn ensure_pod(&self, session_id: &str) -> Result<PodStatus, kube::Error> {
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

        // Create new pod.
        let pod = pod_spec::build_agent_pod(session_id, &self.default_config);
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
    pub async fn reap_idle_pods(&self) {
        let pods: Api<Pod> = Api::namespaced(self.kube.clone(), &self.namespace);
        let lp = ListParams::default().labels(&format!("app={LABEL_APP_VALUE}"));

        let list = match pods.list(&lp).await {
            Ok(l) => l,
            Err(e) => {
                error!(error = %e, "Failed to list agent pods for reaping");
                return;
            }
        };

        let now = time::OffsetDateTime::now_utc();

        for pod in &list.items {
            let name = match &pod.metadata.name {
                Some(n) => n.clone(),
                None => continue,
            };

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
                        let _ = pods.delete(&name, &DeleteParams::default()).await;
                        continue;
                    }
                }
            }

            // Check idle timeout from last-activity annotation.
            let last_activity = pod
                .metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get(ANNOTATION_LAST_ACTIVITY))
                .and_then(|ts| {
                    time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339)
                        .ok()
                });

            if let Some(last) = last_activity {
                let idle = now - last;
                if idle.unsigned_abs() > IDLE_TIMEOUT {
                    info!(pod_name = %name, idle_secs = idle.whole_seconds(), "Reaping idle agent pod");
                    let _ = pods.delete(&name, &DeleteParams::default()).await;
                }
            }
        }
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
            .base_url(format!("http://{pod_ip}:{OPENCODE_PORT}"))
            .timeout_secs(120)
            .build()
    }
}
