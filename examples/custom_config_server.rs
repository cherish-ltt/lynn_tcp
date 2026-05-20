//! Custom configuration TCP server example
//!
//! This example demonstrates how to customize the Lynn TCP server configuration
//! using `LynnServerConfigBuilder`:
//! - Custom listen address: 0.0.0.0:9876
//! - Max connections: 500
//! - Max task pool size: 256
//! - TCP keep-alive enabled (120 seconds)
//! - TCP_NODELAY enabled
//! - Custom heartbeat check interval (10s) and timeout (60s)
//!
//! # Run
//!
//! ```bash
//! cargo run --example custom_config_server
//! ```
//!
//! # Behavior
//!
//! - Same handler patterns as basic_server, but running on a custom configuration
//! - Listens on 0.0.0.0:9876

use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    fmt::init();

    // Build custom server configuration
    let max_connections = 500_usize;
    let max_taskpool = 256_usize;
    let keepalive_time = 120_u64;
    let heart_interval = 10_u64;
    let heart_timeout = 60_u64;
    let tcp_nodelay = true;
    let tcp_keepalive_enabled = true;

    let config = LynnServerConfigBuilder::new()
        .with_addr("0.0.0.0:9876")?
        .with_server_max_connections(Some(&max_connections))
        .with_server_max_taskpool_size(&max_taskpool)
        .with_tcp_keepalive_enabled(&tcp_keepalive_enabled)
        .with_tcp_keepalive_time_secs(&keepalive_time)
        .with_tcp_nodelay(&tcp_nodelay)
        .with_server_check_heart_interval(&heart_interval)
        .with_server_check_heart_timeout_time(&heart_timeout)
        .build();

    println!("🚀 Starting custom-configured Lynn TCP server...");
    println!("   Address:            0.0.0.0:9876");
    println!("   Max connections:    {}", max_connections);
    println!("   Max task pool:      {}", max_taskpool);
    println!(
        "   TCP keep-alive:     {} ({}s)",
        tcp_keepalive_enabled, keepalive_time
    );
    println!("   TCP_NODELAY:        {}", tcp_nodelay);
    println!("   Heartbeat interval: {}s", heart_interval);
    println!("   Heartbeat timeout:  {}s", heart_timeout);
    println!();

    let _server = LynnServer::new_with_config(config)
        .await
        .add_router(1, custom_ping_handler)
        .add_router(2, custom_echo_handler)
        .add_router(3, custom_broadcast_handler)
        .start()
        .await;

    Ok(())
}

/// A simple ping handler using the no-parameter signature.
pub async fn custom_ping_handler() -> HandlerResult {
    println!("📡 [custom_ping_handler] Ping received on custom server");
    HandlerResult::new_without_send()
}

/// An echo handler that receives the client's message buffer.
pub async fn custom_echo_handler(mut input_buf_vo: InputBufVO) -> HandlerResult {
    let client_addr = input_buf_vo
        .get_input_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let all_bytes = input_buf_vo.get_all_bytes();
    let method_id = input_buf_vo.get_method_id();
    println!(
        "📨 [custom_echo_handler] From [{}], method_id={:?}, data_len={}",
        client_addr,
        method_id,
        all_bytes.len()
    );
    HandlerResult::new_without_send()
}

/// A broadcast handler that sends a message to all connected clients.
pub async fn custom_broadcast_handler(clients_context: ClientsContext) -> HandlerResult {
    let addrs = clients_context.get_all_clients_addrs().await;
    println!(
        "📢 [custom_broadcast_handler] Connected clients: {:?}",
        addrs
    );

    if !addrs.is_empty() {
        HandlerResult::new_with_send(3, "Hello from custom Lynn TCP server!".into(), addrs)
    } else {
        HandlerResult::new_without_send()
    }
}
