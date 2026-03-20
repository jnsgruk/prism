use tracing::warn;

/// Maximum retries for transient errors (502, 503, timeouts, etc.).
const MAX_TRANSIENT_RETRIES: u32 = 3;

/// Retry a fallible async operation with exponential backoff for transient errors.
///
/// `is_transient` classifies the error; non-transient errors short-circuit immediately.
/// Returns `Ok(T)` on success, or the last `Err(E)` after exhausting retries.
pub async fn retry_transient<T, E, F, Fut>(
    label: &str,
    is_transient: fn(&E) -> bool,
    f: F,
) -> Result<T, E>
where
    E: std::fmt::Display,
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
{
    let mut last_err;
    match f().await {
        Ok(v) => return Ok(v),
        Err(e) if !is_transient(&e) => return Err(e),
        Err(e) => last_err = e,
    }

    for attempt in 1..=MAX_TRANSIENT_RETRIES {
        let backoff = std::time::Duration::from_secs(2u64.pow(attempt - 1));
        warn!(
            error = %last_err,
            attempt,
            backoff_secs = backoff.as_secs(),
            "{label}: transient error, retrying"
        );
        tokio::time::sleep(backoff).await;

        match f().await {
            Ok(v) => return Ok(v),
            Err(e) if !is_transient(&e) => return Err(e),
            Err(e) => last_err = e,
        }
    }

    Err(last_err)
}
