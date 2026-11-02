use std::{future::Future, io, time::Duration};

pub(crate) async fn with_io_timeout<T>(
    seconds: u64,
    operation: impl Future<Output = io::Result<T>>,
) -> io::Result<T> {
    if seconds == 0 {
        return operation.await;
    }
    tokio::time::timeout(Duration::from_secs(seconds), operation)
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "configured I/O timeout elapsed"))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn timeout_wrapper_preserves_results_and_errors() {
        for seconds in [0, 1] {
            assert_eq!(
                with_io_timeout(seconds, async { Ok(42) }).await.unwrap(),
                42
            );
            let error =
                with_io_timeout::<()>(seconds, async { Err(io::ErrorKind::BrokenPipe.into()) })
                    .await
                    .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        }
    }

    #[tokio::test]
    async fn expired_operation_is_an_explicit_timeout_error() {
        let error = with_io_timeout::<()>(1, std::future::pending())
            .await
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    }
}
