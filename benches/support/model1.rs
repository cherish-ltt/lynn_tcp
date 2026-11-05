use std::time::{Duration, Instant};

use lynn_tcp::{
    lynn_client::LynnClient,
    lynn_tcp_dependents::{HandlerResult, InputBufVOTrait},
};
use tokio::time::timeout;

/// Accept late replies to requests sent inside the measurement window.
pub(crate) const DRAIN: Duration = Duration::from_millis(500);

pub(crate) struct ClientResult {
    pub(crate) requests: u64,
    pub(crate) responses: u64,
    pub(crate) rtts_ns: Vec<u64>,
    pub(crate) exit: &'static str,
    pub(crate) failure_in_warmup: bool,
}

/// Runs one model-1 client: one request in flight, measure every RTT.
pub(crate) async fn model1_client(
    mut client: LynnClient<'static>,
    request: HandlerResult,
    payload: usize,
    measure_start: Instant,
    deadline: Instant,
) -> ClientResult {
    let mut result = ClientResult {
        requests: 0,
        responses: 0,
        rtts_ns: Vec::new(),
        exit: "window_complete",
        failure_in_warmup: false,
    };
    loop {
        let sent_at = Instant::now();
        if sent_at >= deadline {
            break;
        }
        result.failure_in_warmup = sent_at < measure_start;
        let wait = (deadline + DRAIN).saturating_duration_since(Instant::now());
        result.exit = match timeout(wait, client.send_data(request.clone())).await {
            Ok(Ok(())) => "window_complete",
            Ok(Err(_)) => "send_failed",
            Err(_) => "send_timeout",
        };
        if result.exit != "window_complete" {
            break;
        }
        if sent_at >= measure_start {
            result.requests += 1;
        }
        let wait = (deadline + DRAIN).saturating_duration_since(Instant::now());
        result.exit = match timeout(wait, client.get_receive_data()).await {
            Ok(Some(response)) => {
                let bytes = response.get_all_bytes();
                if bytes.len() != payload || bytes.iter().any(|byte| *byte != b'a') {
                    result.exit = "payload_mismatch";
                    break;
                }
                if sent_at >= measure_start {
                    result.responses += 1;
                    result.rtts_ns.push(sent_at.elapsed().as_nanos() as u64);
                }
                "window_complete"
            },
            Ok(None) => "receive_closed",
            Err(_) => "receive_timeout",
        };
        if result.exit != "window_complete" {
            break;
        }
    }
    if result.exit == "window_complete" {
        result.failure_in_warmup = false;
    }
    result
}
