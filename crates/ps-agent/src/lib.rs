//! Agent container lifecycle management for Prism.
//!
//! Manages ephemeral K8s Pods running `OpenCode` agent containers, maps
//! `OpenCode` SSE events to proto messages, and builds Pod specifications.

pub mod container_manager;
pub mod event_mapper;
pub mod pod_spec;

pub use container_manager::{ContainerManager, OPENCODE_PORT, PodStatus};
pub use pod_spec::AgentPodConfig;

// Re-export opencode_sdk types needed by consumers.
pub use opencode_sdk;
