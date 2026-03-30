pub mod handler;

pub use handler::{MetricsComputeHandler, MetricsComputeHandlerImpl};

use restate_sdk::endpoint::Builder;

use crate::infra::SharedState;

/// Bind the metrics compute handler to the Restate endpoint.
pub fn bind(endpoint: Builder, state: &SharedState) -> Builder {
    let metrics_compute = MetricsComputeHandlerImpl {
        state: state.clone(),
    };
    endpoint.bind(metrics_compute.serve())
}
