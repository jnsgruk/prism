/// Normalise an endpoint URL for `ETag` caching.
///
/// Strips query parameters that change between runs (like `since`, `page`)
/// so the same logical endpoint maps to the same cache key.
pub fn normalise_endpoint(url: &str) -> String {
    let Some(base) = url.split('?').next() else {
        return url.to_string();
    };
    base.to_string()
}
