//! Multi-route service and routing validation example
//!
//! This example demonstrates:
//! - Registering multiple route handlers on the same server
//! - Each route (method_id) being handled by its corresponding handler
//! - Server behavior when an unknown method_id is received (logs warning)
//! - How clients can use timeout to detect missing responses for unknown routes
//!
//! # Run
//!
//! ```bash
//! cargo run --example multi_route_service
//! ```
//!
//! # Behavior
//!
//! - Server registers 3 routes:
//!   - Route 1 (login_handler): Parses login payload, responds with "login success"
//!   - Route 2 (user_info_handler): Parses user info request, responds with user data
//!   - Route 3 (logout_handler): Parses logout payload, responds with "logout success"
//! - Client sends 3 test messages:
//!   1. method_id=1, payload="login:alice"  → handled by login_handler
//!   2. method_id=2, payload="get_user_info" → handled by user_info_handler
//!   3. method_id=99, payload="unknown"      → Server logs "router_map_async no method match,99"
//!                                        → Client uses timeout to handle missing response

use std::time::Duration;

use lynn_tcp::{lynn_client::LynnClient, lynn_server::*, lynn_tcp_dependents::*};
use tracing_subscriber::fmt;

/// Login handler — processes login requests.
///
/// This handler demonstrates how to parse a payload and return a structured response.
pub async fn login_handler(input_buf_vo: InputBufVO) -> HandlerResult {
    let client_addr = input_buf_vo
        .get_input_addr()
        .expect("expected client address");

    let payload = input_buf_vo.get_all_bytes();
    let payload_str = String::from_utf8_lossy(&payload);

    println!(
        "🔐 [login_handler] Login request from [{}]: {:?}",
        client_addr, payload_str
    );

    // Parse the login info (expected format: "login:<username>")
    let username = if let Some(name) = payload_str.strip_prefix("login:") {
        name.trim()
    } else {
        "unknown"
    };

    let response = format!("login success: welcome {}", username);
    println!("🔐 [login_handler] Responding: {}", response);

    HandlerResult::new_with_send(1, response.into(), vec![client_addr])
}

/// User info handler — processes user information requests.
///
/// This handler demonstrates returning different data based on the request payload.
pub async fn user_info_handler(input_buf_vo: InputBufVO) -> HandlerResult {
    let client_addr = input_buf_vo
        .get_input_addr()
        .expect("expected client address");

    let payload = input_buf_vo.get_all_bytes();
    let payload_str = String::from_utf8_lossy(&payload);

    println!(
        "👤 [user_info_handler] User info request from [{}]: {:?}",
        client_addr, payload_str
    );

    // Return mock user info data
    let response = format!(
        "{{ \"username\": \"alice\", \"role\": \"admin\", \"request\": \"{}\" }}",
        payload_str
    );
    println!("👤 [user_info_handler] Responding: {}", response);

    HandlerResult::new_with_send(2, response.into(), vec![client_addr])
}

/// Logout handler — processes logout requests.
pub async fn logout_handler(input_buf_vo: InputBufVO) -> HandlerResult {
    let client_addr = input_buf_vo
        .get_input_addr()
        .expect("expected client address");

    let payload = input_buf_vo.get_all_bytes();
    let payload_str = String::from_utf8_lossy(&payload);

    println!(
        "🚪 [logout_handler] Logout request from [{}]: {:?}",
        client_addr, payload_str
    );

    let response = "logout success".to_string();
    println!("🚪 [logout_handler] Responding: {}", response);

    HandlerResult::new_with_send(3, response.into(), vec![client_addr])
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    fmt::init();

    println!("🚀 Starting multi-route service example...");
    println!();

    // ===== Step 1: Start the server in a background task =====
    let _server_handle = tokio::spawn(async {
        let _server = LynnServer::new()
            .await
            .add_router(1, login_handler)
            .add_router(2, user_info_handler)
            .add_router(3, logout_handler)
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

    // ===== Test 1: Send login request (route 1) =====
    println!("--- Test 1: Login (route 1) ---");
    client
        .send_data(HandlerResult::new_with_send_to_server(
            1,
            "login:alice".into(),
        ))
        .await?;

    if let Some(response) = client.get_receive_data().await {
        let payload = response.get_all_bytes();
        let payload_str = String::from_utf8_lossy(&payload);
        println!("✅ Received response: {:?}", payload_str);
    } else {
        println!("❌ No response for login request");
    }
    println!();

    // ===== Test 2: Send user info request (route 2) =====
    println!("--- Test 2: User Info (route 2) ---");
    client
        .send_data(HandlerResult::new_with_send_to_server(
            2,
            "get_user_info".into(),
        ))
        .await?;

    if let Some(response) = client.get_receive_data().await {
        let payload = response.get_all_bytes();
        let payload_str = String::from_utf8_lossy(&payload);
        println!("✅ Received response: {:?}", payload_str);
    } else {
        println!("❌ No response for user info request");
    }
    println!();

    // ===== Test 3: Send unknown route request (route 99) =====
    // This route is not registered on the server.
    // Server will log: "router_map_async no method match,99"
    // Client will NOT receive a response because the server doesn't generate one.
    println!("--- Test 3: Unknown route (method_id=99) ---");
    println!("(Server should log a warning about unknown method_id=99)");
    client
        .send_data(HandlerResult::new_with_send_to_server(
            99,
            "unknown_route".into(),
        ))
        .await?;

    // Use timeout to detect that no response will come for unknown routes
    match tokio::time::timeout(Duration::from_secs(2), client.get_receive_data()).await {
        Ok(Some(response)) => {
            let payload = response.get_all_bytes();
            let payload_str = String::from_utf8_lossy(&payload);
            // Unexpected — unknown routes might get a response in some implementations
            println!("⚠️  Unexpectedly received response: {:?}", payload_str);
        },
        Ok(None) => {
            // Channel closed — this shouldn't normally happen
            println!("⚠️  Client channel closed (unexpected for unknown route behavior)");
        },
        Err(_) => {
            // Timeout! This is the expected behavior:
            // unknown routes produce no response, so get_receive_data() never returns.
            println!("✅ [expected] Timeout — no response for unknown route 99 (as expected)");
        },
    }
    println!();

    // Give a moment for logs to flush, then exit
    tokio::time::sleep(Duration::from_millis(100)).await;
    println!("🏁 Multi-route service example completed successfully!");

    Ok(())
}
