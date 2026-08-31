//! Bounded, cancellation-aware retries for background archive source reads only.
use super::*;
use aws_sdk_s3::{
    error::{ProvideErrorMetadata, SdkError},
    operation::get_object::GetObjectError,
};
use std::future::Future;

pub(super) const ARCHIVE_READ_TIMEOUT: Duration = Duration::from_secs(120);
const MAX_ATTEMPTS: u32 = 3;

#[derive(Debug, thiserror::Error)]
#[error("临时存储读取错误")]
struct TransientRead;

pub(super) fn classify(error: anyhow::Error, retryable: bool) -> anyhow::Error {
    if retryable {
        error.context(TransientRead)
    } else {
        error
    }
}

fn transient_status(status: u16) -> bool {
    matches!(status, 408 | 429 | 500..=599)
}

pub(super) fn is_transient_s3(error: &SdkError<GetObjectError>) -> bool {
    if let Some(response) = error.raw_response() {
        let status = response.status().as_u16();
        if matches!(status, 401 | 403 | 404) {
            return false;
        }
        if transient_status(status) {
            return true;
        }
    }
    match error {
        SdkError::TimeoutError(_) => true,
        SdkError::DispatchFailure(error) => error.is_io() || error.is_timeout(),
        _ => matches!(
            error.as_service_error().and_then(|error| error.code()),
            Some(
                "RequestTimeout"
                    | "RequestTimeoutException"
                    | "SlowDown"
                    | "Throttling"
                    | "ThrottlingException"
                    | "InternalError"
                    | "InternalFailure"
                    | "ServiceUnavailable"
            )
        ),
    }
}

fn is_transient(error: &anyhow::Error) -> bool {
    if error.downcast_ref::<TransientRead>().is_some()
        || error
            .downcast_ref::<tokio::time::error::Elapsed>()
            .is_some()
    {
        return true;
    }
    if let Some(error) = error.downcast_ref::<reqwest::Error>() {
        return error.is_timeout()
            || error.is_connect()
            || error.is_body()
            || error
                .status()
                .is_some_and(|status| transient_status(status.as_u16()));
    }
    error.downcast_ref::<std::io::Error>().is_some_and(|error| {
        matches!(
            error.kind(),
            ErrorKind::TimedOut
                | ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::Interrupted
                | ErrorKind::BrokenPipe
                | ErrorKind::UnexpectedEof
        )
    })
}

pub(super) async fn read_for_archive<F, Fut>(
    token: &CancellationToken,
    photo_id: &str,
    mut read: F,
) -> Result<Vec<u8>>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Vec<u8>>>,
{
    for attempt in 1..=MAX_ATTEMPTS {
        let result = tokio::select! {
            biased;
            _ = token.cancelled() => bail!("打包已取消"),
            result = timeout(ARCHIVE_READ_TIMEOUT, read()) => {
                result.context("读取打包原图超时（单次最多 120 秒）").and_then(|result| result)
            },
        };
        match result {
            Ok(bytes) => return Ok(bytes),
            Err(error) => {
                if attempt == MAX_ATTEMPTS || !is_transient(&error) {
                    return Err(error).with_context(|| {
                        format!("读取图片 {photo_id} 失败（已尝试 {attempt} 次）")
                    });
                }
                // Jitter avoids all four workers retrying together after a remote hiccup.
                let delay = Duration::from_secs(1 << (attempt - 1))
                    + Duration::from_millis((OsRng.next_u32() % 251) as u64);
                warn!(
                    photo_id,
                    attempt,
                    retry_in_ms = delay.as_millis() as u64,
                    "temporary archive source read failure; retrying"
                );
                tokio::select! {
                    biased;
                    _ = token.cancelled() => bail!("打包已取消"),
                    _ = tokio::time::sleep(delay) => {},
                }
            }
        }
    }
    unreachable!("bounded nonempty retry loop")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test(start_paused = true)]
    async fn temporary_errors_retry_the_same_photo_with_backoff() {
        let calls = AtomicU32::new(0);
        let start = tokio::time::Instant::now();
        let result = read_for_archive(&CancellationToken::new(), "photo", || async {
            if calls.fetch_add(1, Ordering::SeqCst) < 2 {
                Err(std::io::Error::from(ErrorKind::ConnectionReset).into())
            } else {
                Ok(vec![1, 2, 3])
            }
        })
        .await
        .unwrap();
        assert_eq!(result, [1, 2, 3]);
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!((Duration::from_secs(3)..=Duration::from_millis(3505)).contains(&start.elapsed()));
    }

    #[tokio::test(start_paused = true)]
    async fn slow_reads_get_more_than_thirty_seconds() {
        let result = read_for_archive(&CancellationToken::new(), "photo", || async {
            tokio::time::sleep(Duration::from_secs(45)).await;
            Ok(vec![7])
        })
        .await
        .unwrap();
        assert_eq!(result, [7]);
    }

    #[tokio::test(start_paused = true)]
    async fn stalled_reads_exhaust_three_bounded_attempts() {
        let calls = AtomicU32::new(0);
        let start = tokio::time::Instant::now();
        let error = read_for_archive(&CancellationToken::new(), "stalled-photo", || {
            calls.fetch_add(1, Ordering::SeqCst);
            std::future::pending::<Result<Vec<u8>>>()
        })
        .await
        .unwrap_err();
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert!(error.to_string().contains("stalled-photo"));
        assert!(error.to_string().contains("3 次"));
        assert!((Duration::from_secs(363)..=Duration::from_secs(364)).contains(&start.elapsed()));
    }

    #[tokio::test(start_paused = true)]
    async fn permanent_errors_are_not_retried() {
        for kind in [
            ErrorKind::NotFound,
            ErrorKind::PermissionDenied,
            ErrorKind::InvalidData,
        ] {
            let calls = AtomicU32::new(0);
            assert!(
                read_for_archive(&CancellationToken::new(), "photo", || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Err(std::io::Error::from(kind).into())
                })
                .await
                .is_err()
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_interrupts_an_inflight_read_and_retry_backoff() {
        for in_backoff in [false, true] {
            let token = CancellationToken::new();
            let cancel = token.clone();
            let calls = AtomicU32::new(0);
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(100)).await;
                cancel.cancel();
            });
            let start = tokio::time::Instant::now();
            let error = read_for_archive(&token, "photo", || async {
                calls.fetch_add(1, Ordering::SeqCst);
                if in_backoff {
                    Err(std::io::Error::from(ErrorKind::TimedOut).into())
                } else {
                    std::future::pending().await
                }
            })
            .await
            .unwrap_err();
            assert_eq!(error.to_string(), "打包已取消");
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            assert!(start.elapsed() < Duration::from_secs(1));
        }
    }

    #[tokio::test]
    async fn cancelled_job_does_not_start_another_request() {
        let token = CancellationToken::new();
        token.cancel();
        let calls = AtomicU32::new(0);
        assert!(
            read_for_archive(&token, "photo", || async {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(vec![1])
            })
            .await
            .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
