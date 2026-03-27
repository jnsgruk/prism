//! Manages ephemeral K8s Pods running `OpenCode` agent containers.
//!
//! Each conversation gets its own Pod. Pods are created on demand, reused for
//! follow-up questions, and reaped when idle or after a maximum lifetime.

use k8s_openapi::api::core::v1::{
    PersistentVolumeClaim, PersistentVolumeClaimSpec, Pod, VolumeResourceRequirements,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use kube::Client as KubeClient;
use kube::api::{Api, DeleteParams, ListParams, PostParams};

use crate::pod_spec::{
    ANNOTATION_LAST_ACTIVITY, ANNOTATION_TOKEN_SESSION_ID, AgentPodConfig, LABEL_APP_VALUE,
    LABEL_SESSION,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

/// Size of each workspace PVC.
const WORKSPACE_PVC_SIZE: &str = "5Gi";

/// Label value used to identify workspace PVCs.
const PVC_LABEL_APP_VALUE: &str = "prism-agent-workspace";

/// Compute the PVC name for a given session (conversation) ID.
pub fn pvc_name_for_session(session_id: &str) -> String {
    format!("prism-ws-{}", &session_id[..8.min(session_id.len())])
}

/// Port exposed by the `OpenCode` server inside each agent container.
pub const OPENCODE_PORT: u16 = 4096;

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
        // Ensure workspace PVC exists for this conversation.
        let pvc_name = self.ensure_pvc(session_id).await?;
        let mut pod = crate::pod_spec::build_agent_pod(session_id, &config, Some(&pvc_name));

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
    /// Returns the token session IDs from reaped pods so the caller can delete
    /// the corresponding auth sessions.
    pub async fn reap_idle_pods(&self) -> Vec<String> {
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
        let mut reaped_token_sessions = Vec::new();

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
                // Collect token session ID before deleting the pod.
                if let Some(token_sid) = pod
                    .metadata
                    .annotations
                    .as_ref()
                    .and_then(|a| a.get(ANNOTATION_TOKEN_SESSION_ID))
                {
                    reaped_token_sessions.push(token_sid.clone());
                }
                let _ = pods.delete(&name, &DeleteParams::default()).await;
            }
        }

        reaped_token_sessions
    }

    /// Count currently active (Pending + Running) agent Pods.
    async fn count_active_pods(&self) -> Result<usize, kube::Error> {
        let pods: Api<Pod> = Api::namespaced(self.kube.clone(), &self.namespace);
        let lp = ListParams::default().labels(&format!("app={LABEL_APP_VALUE}"));
        let list = pods.list(&lp).await?;
        Ok(list.items.len())
    }

    /// Ensure a workspace PVC exists for the given session, creating it if needed.
    ///
    /// Returns the PVC name. Idempotent — returns the existing name if already present.
    pub async fn ensure_pvc(&self, session_id: &str) -> Result<String, kube::Error> {
        let name = pvc_name_for_session(session_id);
        let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(self.kube.clone(), &self.namespace);

        // Check if PVC already exists.
        match pvcs.get(&name).await {
            Ok(_) => return Ok(name),
            Err(kube::Error::Api(ref e)) if e.code == 404 => {}
            Err(e) => return Err(e),
        }

        let pvc = PersistentVolumeClaim {
            metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
                name: Some(name.clone()),
                namespace: Some(self.namespace.clone()),
                labels: Some(BTreeMap::from([
                    (
                        crate::pod_spec::LABEL_APP.to_string(),
                        PVC_LABEL_APP_VALUE.to_string(),
                    ),
                    (LABEL_SESSION.to_string(), session_id.to_string()),
                ])),
                ..Default::default()
            },
            spec: Some(PersistentVolumeClaimSpec {
                access_modes: Some(vec!["ReadWriteOnce".to_string()]),
                resources: Some(VolumeResourceRequirements {
                    requests: Some(BTreeMap::from([(
                        "storage".to_string(),
                        Quantity(WORKSPACE_PVC_SIZE.to_string()),
                    )])),
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        pvcs.create(&PostParams::default(), &pvc).await?;
        info!(session_id, pvc_name = %name, "Created workspace PVC");
        Ok(name)
    }

    /// Delete the workspace PVC for a session.
    pub async fn delete_pvc(&self, session_id: &str) -> Result<(), kube::Error> {
        let name = pvc_name_for_session(session_id);
        let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(self.kube.clone(), &self.namespace);

        match pvcs.delete(&name, &DeleteParams::default()).await {
            Ok(_) => info!(pvc_name = %name, session_id, "Deleted workspace PVC"),
            Err(kube::Error::Api(ref e)) if e.code == 404 => {}
            Err(e) => warn!(pvc_name = %name, error = %e, "Failed to delete workspace PVC"),
        }
        Ok(())
    }

    /// List all workspace PVCs. Returns `(pvc_name, session_id)` pairs.
    pub async fn list_workspace_pvcs(&self) -> Result<Vec<(String, String)>, kube::Error> {
        let pvcs: Api<PersistentVolumeClaim> = Api::namespaced(self.kube.clone(), &self.namespace);
        let lp = ListParams::default().labels(&format!("app={PVC_LABEL_APP_VALUE}"));
        let list = pvcs.list(&lp).await?;

        Ok(list
            .items
            .iter()
            .filter_map(|pvc| {
                let name = pvc.metadata.name.clone()?;
                let session_id = pvc.metadata.labels.as_ref()?.get(LABEL_SESSION)?.clone();
                Some((name, session_id))
            })
            .collect())
    }

    /// Create an `OpenCode` SDK client pointing at the given Pod IP.
    pub fn opencode_client(
        pod_ip: &str,
    ) -> Result<opencode_sdk::Client, opencode_sdk::OpencodeError> {
        opencode_sdk::ClientBuilder::new()
            .base_url(format!("http://{pod_ip}:{OPENCODE_PORT}"))
            .directory("/home/agent")
            .timeout_secs(120)
            .build()
    }
}
