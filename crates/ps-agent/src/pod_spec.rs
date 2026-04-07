//! Pod specification builder for agent containers.

use k8s_openapi::api::core::v1::{
    Container, EnvVar, PersistentVolumeClaimVolumeSource, Pod, PodSpec, ResourceRequirements,
    Volume, VolumeMount,
};
use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
use std::collections::BTreeMap;

/// Configuration for building an agent Pod spec.
#[derive(Clone)]
pub struct AgentPodConfig {
    /// Container image for the agent (e.g. `prism-agent:latest`).
    pub image: String,
    /// Namespace where agent pods are created.
    pub namespace: String,
    /// AI model identifier (e.g. `anthropic/claude-sonnet-4-6`).
    pub model: String,
    /// Small model for summarisation tasks.
    pub small_model: String,
    /// gRPC URL for ps-server (e.g. `http://ps-server:8080`).
    pub prism_api_url: String,
    /// Service account token for the MCP server.
    pub service_token: String,
    /// S3 endpoint for artifact uploads (e.g. `http://rustfs:9000`).
    pub s3_endpoint: String,
    /// S3 bucket name.
    pub s3_bucket: String,
    /// S3 access key ID for artifact uploads.
    pub s3_access_key_id: String,
    /// S3 secret access key for artifact uploads.
    pub s3_secret_access_key: String,
    /// Provider API keys: `(env_var_name, value)`.
    pub provider_keys: Vec<(String, String)>,
}

/// Labels applied to every agent Pod.
pub const LABEL_APP: &str = "app";
pub const LABEL_APP_VALUE: &str = "prism-agent";
pub const LABEL_SESSION: &str = "prism.canonical.com/session";
pub const ANNOTATION_LAST_ACTIVITY: &str = "prism.canonical.com/last-activity";
pub const ANNOTATION_TOKEN_SESSION_ID: &str = "prism.canonical.com/token-session-id";
pub const WORKSPACE_VOLUME_NAME: &str = "workspace";
pub const WORKSPACE_MOUNT_PATH: &str = "/workspace";

/// Build a K8s Pod spec for an agent container.
///
/// When `pvc_name` is provided, a persistent volume is mounted at `/workspace`.
/// When `None`, the workspace is ephemeral container storage.
pub fn build_agent_pod(session_id: &str, config: &AgentPodConfig, pvc_name: Option<&str>) -> Pod {
    let labels = BTreeMap::from([
        (LABEL_APP.to_string(), LABEL_APP_VALUE.to_string()),
        (LABEL_SESSION.to_string(), session_id.to_string()),
    ]);

    let annotations = BTreeMap::from([(
        ANNOTATION_LAST_ACTIVITY.to_string(),
        time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    )]);

    let mut env = vec![
        env_var("SESSION_ID", session_id),
        env_var("OPENCODE_MODEL", &config.model),
        env_var("OPENCODE_SMALL_MODEL", &config.small_model),
        env_var("PRISM_API_URL", &config.prism_api_url),
        env_var("SERVICE_TOKEN", &config.service_token),
        env_var("S3_ENDPOINT", &config.s3_endpoint),
        env_var("S3_BUCKET", &config.s3_bucket),
        env_var("AWS_ACCESS_KEY_ID", &config.s3_access_key_id),
        env_var("AWS_SECRET_ACCESS_KEY", &config.s3_secret_access_key),
        env_var("AWS_DEFAULT_REGION", "us-east-1"),
        env_var("RUST_LOG", "ps_mcp=debug,info"),
    ];

    for (name, value) in &config.provider_keys {
        env.push(env_var(name, value));
    }

    let resources = ResourceRequirements {
        requests: Some(BTreeMap::from([
            ("cpu".to_string(), Quantity("250m".to_string())),
            ("memory".to_string(), Quantity("512Mi".to_string())),
        ])),
        limits: Some(BTreeMap::from([
            ("cpu".to_string(), Quantity("1000m".to_string())),
            ("memory".to_string(), Quantity("2Gi".to_string())),
            ("ephemeral-storage".to_string(), Quantity("2Gi".to_string())),
        ])),
        ..Default::default()
    };

    let volume_mounts = pvc_name.map(|_| {
        vec![VolumeMount {
            name: WORKSPACE_VOLUME_NAME.to_string(),
            mount_path: WORKSPACE_MOUNT_PATH.to_string(),
            ..Default::default()
        }]
    });

    let container = Container {
        name: "agent".to_string(),
        image: Some(config.image.clone()),
        image_pull_policy: Some("IfNotPresent".to_string()),
        env: Some(env),
        resources: Some(resources),
        ports: Some(vec![k8s_openapi::api::core::v1::ContainerPort {
            container_port: 4096,
            name: Some("opencode".to_string()),
            ..Default::default()
        }]),
        volume_mounts,
        ..Default::default()
    };

    let volumes = pvc_name.map(|name| {
        vec![Volume {
            name: WORKSPACE_VOLUME_NAME.to_string(),
            persistent_volume_claim: Some(PersistentVolumeClaimVolumeSource {
                claim_name: name.to_string(),
                read_only: Some(false),
            }),
            ..Default::default()
        }]
    });

    Pod {
        metadata: k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta {
            name: Some(format!(
                "prism-agent-{}",
                &session_id[..8.min(session_id.len())]
            )),
            namespace: Some(config.namespace.clone()),
            labels: Some(labels),
            annotations: Some(annotations),
            ..Default::default()
        },
        spec: Some(PodSpec {
            containers: vec![container],
            restart_policy: Some("Never".to_string()),
            volumes,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn env_var(name: &str, value: &str) -> EnvVar {
    EnvVar {
        name: name.to_string(),
        value: Some(value.to_string()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> AgentPodConfig {
        AgentPodConfig {
            image: "prism-agent:latest".to_string(),
            namespace: "prism".to_string(),
            model: "anthropic/claude-sonnet-4-6".to_string(),
            small_model: "anthropic/claude-haiku-4-5".to_string(),
            prism_api_url: "http://ps-server:8080".to_string(),
            service_token: "test-token-123".to_string(),
            s3_endpoint: "http://rustfs:9000".to_string(),
            s3_bucket: "ps-artifacts".to_string(),
            s3_access_key_id: "test-key".to_string(),
            s3_secret_access_key: "test-secret".to_string(),
            provider_keys: vec![("ANTHROPIC_API_KEY".to_string(), "sk-ant-test".to_string())],
        }
    }

    #[test]
    fn pod_has_correct_labels() {
        let pod = build_agent_pod("sess-abc123", &test_config(), None);
        let labels = pod.metadata.labels.as_ref().unwrap();
        assert_eq!(labels.get(LABEL_APP), Some(&"prism-agent".to_string()));
        assert_eq!(labels.get(LABEL_SESSION), Some(&"sess-abc123".to_string()));
    }

    #[test]
    fn pod_has_last_activity_annotation() {
        let pod = build_agent_pod("sess-abc123", &test_config(), None);
        let annotations = pod.metadata.annotations.as_ref().unwrap();
        assert!(annotations.contains_key(ANNOTATION_LAST_ACTIVITY));
    }

    #[test]
    fn pod_name_uses_session_prefix() {
        let pod = build_agent_pod("sess-abc123", &test_config(), None);
        assert_eq!(pod.metadata.name, Some("prism-agent-sess-abc".to_string()));
    }

    #[test]
    fn pod_has_correct_namespace() {
        let pod = build_agent_pod("sess-abc123", &test_config(), None);
        assert_eq!(pod.metadata.namespace, Some("prism".to_string()));
    }

    #[test]
    fn container_has_model_env_vars() {
        let pod = build_agent_pod("sess-abc123", &test_config(), None);
        let container = &pod.spec.as_ref().unwrap().containers[0];
        let env = container.env.as_ref().unwrap();

        let find_env = |name: &str| -> Option<&str> {
            env.iter()
                .find(|e| e.name == name)
                .and_then(|e| e.value.as_deref())
        };

        assert_eq!(
            find_env("OPENCODE_MODEL"),
            Some("anthropic/claude-sonnet-4-6")
        );
        assert_eq!(
            find_env("OPENCODE_SMALL_MODEL"),
            Some("anthropic/claude-haiku-4-5")
        );
        assert_eq!(find_env("SESSION_ID"), Some("sess-abc123"));
        assert_eq!(find_env("SERVICE_TOKEN"), Some("test-token-123"));
        assert_eq!(find_env("ANTHROPIC_API_KEY"), Some("sk-ant-test"));
    }

    #[test]
    fn container_has_resource_limits() {
        let pod = build_agent_pod("sess-abc123", &test_config(), None);
        let container = &pod.spec.as_ref().unwrap().containers[0];
        let resources = container.resources.as_ref().unwrap();

        let limits = resources.limits.as_ref().unwrap();
        assert_eq!(limits.get("cpu"), Some(&Quantity("1000m".to_string())));
        assert_eq!(limits.get("memory"), Some(&Quantity("2Gi".to_string())));
        assert_eq!(
            limits.get("ephemeral-storage"),
            Some(&Quantity("2Gi".to_string()))
        );

        let requests = resources.requests.as_ref().unwrap();
        assert_eq!(requests.get("cpu"), Some(&Quantity("250m".to_string())));
        assert_eq!(requests.get("memory"), Some(&Quantity("512Mi".to_string())));
    }

    #[test]
    fn container_exposes_port_4096() {
        let pod = build_agent_pod("sess-abc123", &test_config(), None);
        let container = &pod.spec.as_ref().unwrap().containers[0];
        let ports = container.ports.as_ref().unwrap();
        assert_eq!(ports[0].container_port, 4096);
    }

    #[test]
    fn restart_policy_is_never() {
        let pod = build_agent_pod("sess-abc123", &test_config(), None);
        assert_eq!(
            pod.spec.as_ref().unwrap().restart_policy,
            Some("Never".to_string())
        );
    }

    #[test]
    fn pod_has_no_volumes_without_pvc() {
        let pod = build_agent_pod("sess-abc123", &test_config(), None);
        let spec = pod.spec.as_ref().unwrap();
        assert!(spec.volumes.is_none());
        let container = &spec.containers[0];
        assert!(container.volume_mounts.is_none());
    }

    #[test]
    fn pod_has_workspace_volume_when_pvc_provided() {
        let pod = build_agent_pod("sess-abc123", &test_config(), Some("prism-ws-sess-abc"));
        let spec = pod.spec.as_ref().unwrap();

        let volumes = spec.volumes.as_ref().expect("volumes should be set");
        assert_eq!(volumes.len(), 1);
        assert_eq!(volumes[0].name, "workspace");
        let pvc_source = volumes[0]
            .persistent_volume_claim
            .as_ref()
            .expect("PVC source should be set");
        assert_eq!(pvc_source.claim_name, "prism-ws-sess-abc");
        assert_eq!(pvc_source.read_only, Some(false));

        let container = &spec.containers[0];
        let mounts = container
            .volume_mounts
            .as_ref()
            .expect("volume mounts should be set");
        assert_eq!(mounts.len(), 1);
        assert_eq!(mounts[0].name, "workspace");
        assert_eq!(mounts[0].mount_path, "/workspace");
    }

    #[test]
    fn provider_keys_are_injected() {
        let mut config = test_config();
        config.provider_keys = vec![
            ("GOOGLE_API_KEY".to_string(), "gk-123".to_string()),
            ("GEMINI_API_KEY".to_string(), "gk-123".to_string()),
        ];
        let pod = build_agent_pod("sess-abc123", &config, None);
        let container = &pod.spec.as_ref().unwrap().containers[0];
        let env = container.env.as_ref().unwrap();

        let google_key = env.iter().find(|e| e.name == "GOOGLE_API_KEY");
        assert!(google_key.is_some());
        assert_eq!(google_key.unwrap().value.as_deref(), Some("gk-123"));
    }
}
