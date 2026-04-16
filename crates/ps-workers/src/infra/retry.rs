use tracing::warn;

/// Maximum retries for transient errors (502, 503, timeouts, etc.).
const MAX_TRANSIENT_RETRIES: u32 = 5;

/// Base delay for exponential backoff (multiplied by 2^(attempt-1)).
const BACKOFF_BASE_SECS: u64 = 8;

/// Ceiling for any single backoff delay.
const MAX_BACKOFF_SECS: u64 = 120;

/// Compute backoff with ±25% jitter clamped to [`MAX_BACKOFF_SECS`].
fn backoff_with_jitter(attempt: u32) -> std::time::Duration {
    let base = (BACKOFF_BASE_SECS * 2u64.pow(attempt - 1)).min(MAX_BACKOFF_SECS);
    let low = base * 3 / 4; // -25%
    let high = (base * 5 / 4).min(MAX_BACKOFF_SECS); // +25%, still capped
    std::time::Duration::from_secs(fastrand::u64(low..=high))
}

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
        let backoff = backoff_with_jitter(attempt);
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

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    #[derive(Debug)]
    struct TestError {
        transient: bool,
        msg: String,
    }

    impl std::fmt::Display for TestError {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "{}", self.msg)
        }
    }

    fn is_transient(e: &TestError) -> bool {
        e.transient
    }

    #[tokio::test]
    async fn succeeds_immediately() {
        let call_count = AtomicU32::new(0);
        let result: Result<&str, TestError> = retry_transient("test", is_transient, || async {
            call_count.fetch_add(1, Ordering::SeqCst);
            Ok("ok")
        })
        .await;
        assert_eq!(result.unwrap(), "ok");
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn non_transient_fails_immediately() {
        let call_count = AtomicU32::new(0);
        let result: Result<(), TestError> = retry_transient("test", is_transient, || async {
            call_count.fetch_add(1, Ordering::SeqCst);
            Err(TestError {
                transient: false,
                msg: "permanent".into(),
            })
        })
        .await;
        assert!(result.is_err());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn transient_succeeds_on_retry() {
        let call_count = AtomicU32::new(0);
        let result: Result<&str, TestError> = retry_transient("test", is_transient, || async {
            let n = call_count.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                Err(TestError {
                    transient: true,
                    msg: "transient".into(),
                })
            } else {
                Ok("recovered")
            }
        })
        .await;
        assert_eq!(result.unwrap(), "recovered");
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn exhausts_retries_after_max_attempts() {
        let call_count = AtomicU32::new(0);
        let result: Result<(), TestError> = retry_transient("test", is_transient, || async {
            call_count.fetch_add(1, Ordering::SeqCst);
            Err(TestError {
                transient: true,
                msg: "always transient".into(),
            })
        })
        .await;
        assert!(result.is_err());
        // Initial call + MAX_TRANSIENT_RETRIES (5) = 6 total calls
        assert_eq!(call_count.load(Ordering::SeqCst), 6);
    }
}
