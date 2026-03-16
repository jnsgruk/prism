use ps_core::ingestion::Source;
use ps_core::models::Platform;

use crate::github::GitHubSource;

/// Create a source adapter for the given platform.
///
/// Returns `None` for unrecognised platforms.
pub fn create_source(platform: &Platform) -> Option<Box<dyn Source>> {
    match platform {
        Platform::Github => Some(Box::new(GitHubSource)),
        _ => None,
    }
}
