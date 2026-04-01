pub mod stages;
pub mod workflow;

pub use workflow::{IngestionPipelineWorkflow, IngestionPipelineWorkflowImpl};

use restate_sdk::endpoint::Builder;

use crate::infra::SharedState;

/// Bind the pipeline workflow to the Restate endpoint.
pub fn bind(endpoint: Builder, state: &SharedState) -> Builder {
    let pipeline = IngestionPipelineWorkflowImpl {
        state: state.clone(),
    };
    endpoint.bind(pipeline.serve())
}
