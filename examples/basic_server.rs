//! Basic TCP server example
//!
//! This example demonstrates the simplest way to create a Lynn TCP server:
//! - Uses default configuration (listens on 0.0.0.0:9177)
//! - Registers three route handlers with different parameter patterns
//!
//! # Run
//!
//! ```bash
//! cargo run --example basic_server
//! ```
//!
//! # Behavior
//!
//! - Route 1 (ping_handler): Logs a ping message, no response sent
//! - Route 2 (echo_handler): Logs the client address and received message, no response sent
//! - Route 3 (broadcast_handler): Logs all connected clients and broadcasts a message to all of them

use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    fmt::init();

    println!("🚀 Starting basic Lynn TCP server on default address 0.0.0.0:9177...");
    println!();

    let _server = LynnServer::new()
        .await
        .add_router(1, ping_handler)
        .add_router(2, echo_handler)
        .add_router(3, broadcast_handler)
        .start()
        .await;

    Ok(())
}

/// A simple handler that takes no parameters and logs a ping message.
///
/// This demonstrates the simplest handler signature: `() -> HandlerResult`.
pub async fn ping_handler() -> HandlerResult {
    println!("📡 [ping_handler] Received ping, no response sent");
    HandlerResult::new_without_send()
}

/// A handler that receives the client's message buffer and logs it.
///
/// This demonstrates the `(InputBufVO) -> HandlerResult` signature.
pub async fn echo_handler(input_buf_vo: InputBufVO) -> HandlerResult {
    let client_addr = input_buf_vo
        .get_input_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let data_len = input_buf_vo.get_all_bytes().len();
    println!(
        "📨 [echo_handler] Received {} bytes from client [{}]",
        data_len, client_addr
    );
    HandlerResult::new_without_send()
}

/// A handler that uses ClientsContext to broadcast a message to all connected clients.
///
/// This demonstrates the `(ClientsContext) -> HandlerResult` signature.
pub async fn broadcast_handler(clients_context: ClientsContext) -> HandlerResult {
    let addrs = clients_context.get_all_clients_addrs().await;
    println!(
        "📢 [broadcast_handler] Broadcasting to {} connected client(s): {:?}",
        addrs.len(),
        addrs
    );

    // Broadcast a "hello from server" message to all clients
    if !addrs.is_empty() {
        HandlerResult::new_with_send(3, "Hello from Lynn TCP server!".into(), addrs)
    } else {
        HandlerResult::new_without_send()
    }
}
