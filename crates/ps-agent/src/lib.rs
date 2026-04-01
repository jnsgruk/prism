//! Agent container lifecycle management for Prism.
//!
//! Manages ephemeral K8s Pods running `OpenCode` agent containers, maps
//! `OpenCode` SSE events to proto messages, and builds Pod specifications.

#[cfg(feature = "kube")]
pub mod container_manager;
pub mod event_mapper;
#[cfg(feature = "kube")]
pub mod pod_spec;

/// The port `OpenCode` listens on inside agent pods.
pub const OPENCODE_PORT: u16 = 4096;

#[cfg(feature = "kube")]
pub use container_manager::{ContainerManager, PodOverrides, PodStatus, pvc_name_for_session};
#[cfg(feature = "kube")]
pub use pod_spec::{ANNOTATION_TOKEN_SESSION_ID, AgentPodConfig, WORKSPACE_MOUNT_PATH};

// Re-export opencode_sdk types needed by consumers.
pub use opencode_sdk;
