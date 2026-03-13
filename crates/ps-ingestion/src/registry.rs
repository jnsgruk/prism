use ps_core::ingestion::Source;

use crate::github::GitHubSource;

/// Create a source adapter for the given source type.
///
/// Returns `None` for unrecognised source types.
pub fn create_source(source_type: &str) -> Option<Box<dyn Source>> {
    match source_type {
        "github" => Some(Box::new(GitHubSource)),
        _ => None,
    }
}
