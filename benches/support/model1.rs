use std::time::{Duration, Instant};

use lynn_tcp::{
    lynn_client::LynnClient,
    lynn_tcp_dependents::{HandlerResult, InputBufVOTrait},
};
use tokio::time::timeout;

/// Accept late replies to requests sent inside the measurement window.
pub(crate) const DRAIN: Duration = Duration::from_millis(500);

/// Runs one model-1 client: one request in flight, measure every RTT.
pub(crate) async fn model1_client(
    mut client: LynnClient<'static>,
    request: HandlerResult,
    payload: usize,
    measure_start: Instant,
    deadline: Instant,
) -> (u64, u64, Vec<u64>) {
    let mut requests = 0u64;
    let mut responses = 0u64;
    let mut rtts = Vec::new();
    loop {
        let sent_at = Instant::now();
        if sent_at >= deadline {
            break;
        }
        if client.send_data(request.clone()).await.is_err() {
            break;
        }
        if sent_at >= measure_start {
            requests += 1;
        }
        let wait = (deadline + DRAIN).saturating_duration_since(sent_at);
        match timeout(wait, client.get_receive_data()).await {
            Ok(Some(response)) => {
                let bytes = response.get_all_bytes();
                assert_eq!(bytes.len(), payload);
                assert!(
                    bytes.iter().all(|byte| *byte == b'a'),
                    "echo payload mismatch"
                );
                if sent_at >= measure_start {
                    responses += 1;
                    rtts.push(sent_at.elapsed().as_nanos() as u64);
                }
            },
            // Timed out past the drain window or the connection died.
            _ => break,
        }
    }
    (requests, responses, rtts)
}
