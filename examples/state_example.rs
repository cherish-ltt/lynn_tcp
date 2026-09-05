//! Global state (`AppState<T>`) example — axum-style dependency injection.
//!
//! Demonstrates:
//! - `LynnServer::with_state(value)` registers a shared state (e.g. a
//!   database handle) once
//! - Handlers declare `AppState<T>` parameters and receive `Arc<T>` per request
//! - Multiple state types coexist on one server
//! - `is_connected()` on the client
//!
//! # Run
//!
//! ```bash
//! cargo run --example state_example
//! ```
//!
//! With SeaORM you would call `.with_db(db_conn)` (feature `seaorm`) and take
//! a `lynn_seaorm::DbConn` parameter — the mechanics are identical.

use std::{collections::HashMap, sync::Mutex, time::Duration};

use lynn_tcp::{
    lynn_client::LynnClient, lynn_server::*, lynn_state::AppState, lynn_tcp_dependents::*,
};
use tracing_subscriber::fmt;

/// A fake "database" the handlers query through `AppState<UserRepo>`.
#[derive(Default)]
struct UserRepo {
    users: Mutex<HashMap<u64, String>>,
}

impl UserRepo {
    fn new() -> Self {
        let mut users = HashMap::new();
        users.insert(1, "lynn".to_string());
        users.insert(2, "hyper".to_string());
        Self {
            users: Mutex::new(users),
        }
    }

    fn find_user(&self, id: u64) -> String {
        self.users
            .lock()
            .map(|users| users.get(&id).cloned().unwrap_or_else(|| "unknown".into()))
            .unwrap_or_else(|_| "locked".into())
    }
}

/// A second, independent state type.
struct AppConfig {
    service_name: String,
}

/// Route 1: two extractor parameters — the state and the request payload.
async fn find_user(repo: AppState<UserRepo>, input_buf_vo: InputBufVO) -> HandlerResult {
    let id: u64 = String::from_utf8_lossy(&input_buf_vo.get_all_bytes())
        .trim()
        .parse()
        .unwrap_or(0);
    let name = repo.find_user(id);
    let addr = input_buf_vo.get_input_addr().unwrap();
    HandlerResult::new_with_send(1, name.into(), vec![addr])
}

/// Route 2: `AppState<T>` dereferences to `T`.
async fn service_info(config: AppState<AppConfig>, input_buf_vo: InputBufVO) -> HandlerResult {
    let addr = input_buf_vo.get_input_addr().unwrap();
    HandlerResult::new_with_send(2, config.service_name.clone().into(), vec![addr])
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    fmt::init();

    println!("🚀 Starting Lynn TCP global-state example...");
    println!();

    // ===== Step 1: start the server with two registered states =====
    let _server_handle = tokio::spawn(async {
        let _server = LynnServer::new()
            .await
            .with_state(UserRepo::new())
            .with_state(AppConfig {
                service_name: "lynn-state-demo".to_string(),
            })
            .add_router(1, find_user)
            .add_router(2, service_info)
            .start()
            .await;
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    // ===== Step 2: connect the client =====
    let mut client = LynnClient::new_with_addr("127.0.0.1:9177")
        .await
        .start()
        .await;
    println!("🔌 [client] connected = {}", client.is_connected());

    // ===== Step 3: query the "database" through the injected state =====
    client
        .send_data(HandlerResult::new_with_send_to_server(1, "1".into()))
        .await?;
    if let Some(response) = client.get_receive_data().await {
        println!(
            "📥 [client] user #1 = {:?}",
            String::from_utf8_lossy(&response.get_all_bytes())
        );
    }

    // ===== Step 4: query the second state type =====
    client
        .send_data(HandlerResult::new_with_send_to_server(2, "info".into()))
        .await?;
    if let Some(response) = client.get_receive_data().await {
        println!(
            "📥 [client] service info = {:?}",
            String::from_utf8_lossy(&response.get_all_bytes())
        );
    }

    tokio::time::sleep(Duration::from_millis(100)).await;
    println!();
    println!("🏁 Global-state example completed successfully!");

    Ok(())
}
