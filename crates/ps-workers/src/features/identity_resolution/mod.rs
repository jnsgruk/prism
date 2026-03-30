pub mod handler;

pub use handler::{IdentityResolutionHandler, IdentityResolutionHandlerImpl};

use restate_sdk::endpoint::Builder;

use crate::infra::SharedState;

/// Bind the identity resolution handler to the Restate endpoint.
pub fn bind(endpoint: Builder, state: &SharedState) -> Builder {
    let identity_resolution = IdentityResolutionHandlerImpl {
        state: state.clone(),
    };
    endpoint.bind(identity_resolution.serve())
}
