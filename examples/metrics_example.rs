//! Server lifecycle, frame and byte metrics are collected automatically.
//! Run: cargo run --example metrics_example
//! Inspect: curl http://127.0.0.1:9091/metrics

use lynn_tcp::{lynn_metrics::*, lynn_server::*, lynn_tcp_dependents::*};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    let mut args = std::env::args().skip(1);
    let server_addr = args.next().unwrap_or_else(|| "127.0.0.1:9177".into());
    let metrics_addr = args.next().unwrap_or_else(|| "127.0.0.1:9091".into());
    let metrics = spawn_metrics_server(MetricsServerConfig {
        bind_addr: metrics_addr,
        enabled: true,
    });
    let config = LynnServerConfigBuilder::new()
        .with_addr(server_addr)?
        .build();
    let result = LynnServer::new_with_config(config)
        .await
        .add_router(1, echo)
        .start()
        .await;
    metrics.abort();
    result?;
    Ok(())
}

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(
        1,
        input.get_all_bytes().into(),
        vec![input.get_input_addr().unwrap()],
    )
    .expect("received payload fits the protocol limit")
}
