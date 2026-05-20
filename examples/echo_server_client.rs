//! Echo server-client full interaction example
//!
//! This example demonstrates a complete request-response cycle between a TCP client and server:
//! - Server listens on 0.0.0.0:9177 (default address)
//! - Registers an echo handler (route 1) that receives messages and replies with "Echo: " prefix
//! - Client connects, sends a message, and receives the echoed response
//!
//! # Run
//!
//! ```bash
//! cargo run --example echo_server_client
//! ```
//!
//! # Behavior
//!
//! - Server starts and listens for incoming connections
//! - Client connects and sends "Hello Lynn!" with method_id=1
//! - Server handler echoes back the message prefixed with "Echo: "
//! - Client receives and prints the response

use std::time::Duration;

use lynn_tcp::{lynn_client::LynnClient, lynn_server::*, lynn_tcp_dependents::*};
use tracing_subscriber::fmt;

/// Echo handler that receives a message and sends it back to the sender with "Echo: " prefix.
///
/// This handler demonstrates the `(InputBufVO, ClientsContext) -> HandlerResult` signature,
/// which allows accessing both the incoming message data and the client context simultaneously.
pub async fn echo_handler(
    input_buf_vo: InputBufVO,
    clients_context: ClientsContext,
) -> HandlerResult {
    let client_addr = input_buf_vo
        .get_input_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Extract the payload bytes (after the header)
    let payload = input_buf_vo.get_all_bytes();
    let payload_str = String::from_utf8_lossy(&payload);
    println!(
        "📨 [echo_handler] Received from [{}]: {:?}",
        client_addr, payload_str
    );

    // Construct the echo response
    let response = format!("Echo: {}", payload_str);

    // Get all connected client addresses to send the response back to the sender
    let addrs = clients_context.get_all_clients_addrs().await;
    println!(
        "📤 [echo_handler] Responding to {} client(s): {:?}",
        addrs.len(),
        addrs
    );

    // Send the response back to all connected clients
    HandlerResult::new_with_send(1, response.into(), addrs)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    fmt::init();

    println!("🚀 Starting Lynn TCP echo server-client example...");
    println!();

    // ===== Step 1: Start the server in a background task =====
    let _server_handle = tokio::spawn(async {
        let _server = LynnServer::new()
            .await
            .add_router(1, echo_handler)
            .start()
            .await;
    });

    // Give the server time to start listening
    tokio::time::sleep(Duration::from_millis(200)).await;

    // ===== Step 2: Create and start the client =====
    let mut client = LynnClient::new_with_addr("127.0.0.1:9177")
        .await
        .start()
        .await;

    // ===== Step 3: Send a message to the server =====
    let message = "Hello Lynn!";
    println!(
        "📤 [client] Sending to server: method_id=1, payload={:?}",
        message
    );
    client
        .send_data(HandlerResult::new_with_send_to_server(1, message.into()))
        .await?;

    // ===== Step 4: Wait for and receive the server's response =====
    println!("⏳ [client] Waiting for server response...");
    if let Some(mut response) = client.get_receive_data().await {
        let method_id = response.get_method_id();
        let payload = response.get_all_bytes();
        let payload_str = String::from_utf8_lossy(&payload);
        println!(
            "📥 [client] Got response: method_id={:?}, payload={:?}",
            method_id, payload_str
        );

        // Verify the response has the expected "Echo: " prefix
        if payload_str.starts_with("Echo: ") {
            println!("✅ [client] Successfully received echoed response!");
        } else {
            println!("⚠️  [client] Response doesn't have expected 'Echo:' prefix");
        }
    } else {
        println!("❌ [client] No response received from server");
    }

    // Give a moment for logs to flush, then exit
    tokio::time::sleep(Duration::from_millis(100)).await;
    println!();
    println!("🏁 Echo server-client example completed successfully!");

    Ok(())
}
