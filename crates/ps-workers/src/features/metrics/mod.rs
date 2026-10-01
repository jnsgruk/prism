pub mod handler;
pub mod historical;
pub mod recovery;

pub use handler::{MetricsComputeHandler, MetricsComputeHandlerImpl};

use historical::HistoricalSnapshotService;
use recovery::SnapshotRefreshHandler;
use restate_sdk::endpoint::Builder;

use crate::infra::SharedState;

/// Bind the metrics compute handler to the Restate endpoint.
pub fn bind(endpoint: Builder, state: &SharedState) -> Builder {
    let metrics_compute = MetricsComputeHandlerImpl {
        state: state.clone(),
    };
    let recovery = recovery::SnapshotRefreshHandlerImpl {
        state: state.clone(),
    };
    let historical = historical::HistoricalSnapshotServiceImpl {
        state: state.clone(),
    };
    endpoint
        .bind(metrics_compute.serve())
        .bind(recovery.serve())
        .bind(historical.serve())
}
