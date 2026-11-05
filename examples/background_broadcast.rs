//! Run with `cargo run --example background_broadcast` and connect a Lynn client on :9177.

use std::time::Duration;

use lynn_tcp::{bytes::Bytes, lynn_server::LynnServer};

#[tokio::main]
async fn main() -> lynn_tcp::Result<()> {
    let server = LynnServer::new().await;
    let broadcast = server.broadcast_handle();
    let mut ticks = tokio::time::interval(Duration::from_secs(5));
    ticks.tick().await;
    let pushes = async move {
        loop {
            ticks.tick().await;
            let report = match broadcast.broadcast(100, Bytes::from_static(b"server status")) {
                Ok(report) => report,
                Err(error) => break Err(error),
            };
            println!(
                "queued={}, full={}, closed={}",
                report.enqueued, report.full, report.closed
            );
        }
    };
    tokio::select! {
        result = server.start() => result,
        result = pushes => result,
    }
}
