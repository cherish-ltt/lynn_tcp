//! Custom message protocol example
//!
//! This example demonstrates how to use custom message header/tail marks
//! with the Lynn TCP server:
//! - Custom header mark: 0x1234
//! - Custom tail mark:   0x4321
//! - Listen address:     0.0.0.0:9178
//!
//! It also shows how to parse incoming messages using `InputBufVOTrait` methods:
//! - `get_constructor_id()` — Read the constructor ID from the message
//! - `get_method_id()` — Read the method/route ID from the message
//! - `get_all_bytes()` — Read the remaining payload bytes
//!
//! # Run
//!
//! ```bash
//! cargo run --example custom_protocol
//! ```
//!
//! # Behavior
//!
//! - Listens on 0.0.0.0:9178
//! - Expects messages with header=0x1234 and tail=0x4321
//! - Parses and logs constructor ID, method ID, and payload content

use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    fmt::init();

    // Custom message marks
    let header_mark: u16 = 0x1234;
    let tail_mark: u16 = 0x4321;

    // Build configuration with custom protocol marks
    let config = LynnServerConfigBuilder::new()
        .with_addr("0.0.0.0:9178")?
        .with_message_header_mark(&header_mark)
        .with_message_tail_mark(&tail_mark)
        .build();

    println!("🚀 Starting Lynn TCP server with custom protocol...");
    println!("   Address:      0.0.0.0:9178");
    println!("   Header mark:  0x{:04X}", header_mark);
    println!("   Tail mark:    0x{:04X}", tail_mark);
    println!();

    let _server = LynnServer::new_with_config(config)
        .await
        .add_router(1, custom_protocol_handler)
        .start()
        .await;

    Ok(())
}

/// Handler that demonstrates parsing a message with custom header/tail marks.
///
/// This handler uses `InputBufVOTrait` methods to inspect the incoming message:
/// - `get_constructor_id()`: the first byte of the message payload
/// - `get_method_id()`: the method/route ID (bytes 1-2)
/// - `get_all_bytes()`: the full payload after the 3-byte header
pub async fn custom_protocol_handler(mut input_buf_vo: InputBufVO) -> HandlerResult {
    let client_addr = input_buf_vo
        .get_input_addr()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // Parse message fields using InputBufVOTrait methods
    let constructor_id = input_buf_vo.get_constructor_id();
    let method_id = input_buf_vo.get_method_id();
    let all_bytes = input_buf_vo.get_all_bytes();
    let payload_len = all_bytes.len();
    let remaining = input_buf_vo.get_remaining_data_len();

    println!(
        "📥 [custom_protocol_handler] Received message from [{}]",
        client_addr
    );
    println!("   Constructor ID:  {:?}", constructor_id);
    println!("   Method ID:       {:?}", method_id);
    println!("   Payload length:  {} bytes", payload_len);
    println!("   Remaining data:  {} bytes", remaining);

    if payload_len > 0 {
        // Try to display the payload as a UTF-8 string if possible
        let payload_str = String::from_utf8_lossy(&all_bytes);
        println!("   Payload content: {:?}", payload_str);
    }

    HandlerResult::new_without_send()
}
