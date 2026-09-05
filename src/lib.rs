#![allow(unused)]
#![allow(private_interfaces)]
#![allow(private_bounds)]
#![allow(non_snake_case)]
#![allow(deprecated)]
// Clippy: allow style warnings that don't affect correctness
#![allow(clippy::module_inception)]
#![allow(clippy::doc_lazy_continuation)]
#![allow(clippy::too_many_arguments)]
#![allow(clippy::declare_interior_mutable_const)]
#![allow(clippy::borrow_interior_mutable_const)]
#![allow(clippy::new_without_default)]
#![allow(clippy::let_underscore_future)]
#![allow(clippy::redundant_guards)]
#![allow(clippy::collapsible_if)]
#![allow(clippy::unused_unit)]
#![allow(clippy::unnecessary_cast)]
//! # Lynn_tcp
//! `Lynn_tcp` is a lightweight TCP server framework
//! ## Keywords
//! **Lightweight**: concise code that is easier to learn and use
//!
//! **Concurrent and Performance**: Based on Tokio's excellent asynchronous performance, it is easy to achieve concurrent processing capabilities for multi-user links
//!
//! **Lower latency**: Design with read and write separation to achieve lower latency
//!
//! **Security**: Code written with strong typing and memory safety in Rust
//! ## Features
//! - **server**: Provide customizable TCP services that can easily achieve multi-user long connections and concurrent processing capabilities, with services for different routes
//! - **client**: Provides a custom TCP client that sends and receives messages to and from a TCP server
//! ## Server
//! Represents a server for the Lynn application.
//!
//! The `LynnServer` struct holds information about the server, including its configuration,
//! client list, router map, and thread pool.
//!
//! ### Example
//! Use default config
//! ```rust,no_run
//! use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let _ = LynnServer::new()
//!         .await
//!         .add_router(1, my_service)
//!         .add_router(2, my_service_with_buf)
//!         .add_router(3, my_service_with_clients)
//!         .start()
//!         .await;
//!     Ok(())
//! }
//!
//! pub async fn my_service() -> HandlerResult {
//!     HandlerResult::new_without_send()
//! }
//! pub async fn my_service_with_buf(input_buf_vo: InputBufVO) -> HandlerResult {
//!     println!(
//!         "service read from :{}",
//!         input_buf_vo.get_input_addr().unwrap()
//!     );
//!     HandlerResult::new_without_send()
//! }
//! pub async fn my_service_with_clients(clients_context: ClientsContext) -> HandlerResult {
//!     HandlerResult::new_with_send(1, "hello lynn".into(), clients_context.get_all_clients_addrs().await)
//! }
//! ```
//! ### Example
//! Use customized config
//! ```rust,no_run
//! use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let _ = LynnServer::new_with_config(
//!         LynnServerConfigBuilder::new()
//!             .with_addr("0.0.0.0:9177").unwrap()
//!             .with_server_max_connections(Some(&200))
//!             // Suggestion 256-512
//!             .with_server_max_taskpool_size(&512)
//!             // ...more
//!             .build(),
//!         )
//!         .await
//!         .add_router(1, my_service)
//!         .add_router(2, my_service_with_buf)
//!         .add_router(3, my_service_with_clients)
//!         .start()
//!         .await;
//!     Ok(())
//! }
//!
//! pub async fn my_service() -> HandlerResult {
//!     HandlerResult::new_without_send()
//! }
//! pub async fn my_service_with_buf(input_buf_vo: InputBufVO) -> HandlerResult {
//!     println!(
//!         "service read from :{}",
//!         input_buf_vo.get_input_addr().unwrap()
//!     );
//!     HandlerResult::new_without_send()
//! }
//! pub async fn my_service_with_clients(clients_context: ClientsContext) -> HandlerResult {
//!     HandlerResult::new_with_send(1, "hello lynn".into(), clients_context.get_all_clients_addrs().await)
//! }
//! ```
//! ## Clinet
//! A client for communicating with a server over TCP.
//!
//! The `LynnClient` struct represents a client that can connect to a server, send data, and receive data.
//! It uses a configuration object to specify the server's IP address and other settings.
//! The client runs in a separate task and uses channels to communicate with the main task.
//! ### Example
//! Use default config (If you want to use custom configuration, please use `LynnClientConfigBuilder`)
//! ```rust,no_run
//! use lynn_tcp::{
//!     lynn_client::LynnClient,
//!     lynn_tcp_dependents::*,
//! };
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut client = LynnClient::new_with_ipv4("127.0.0.1:9177")
//!             .await
//!             .start()
//!             .await;
//!     let _ = client.send_data(HandlerResult::new_with_send_to_server(1, "hello".into())).await;
//!     let input_buf_vo = client.get_receive_data().await.unwrap();
//!     Ok(())
//! }
//! ```

// ===== 分层架构模块注册 =====

/// 应用层（编排层）
pub(crate) mod application;
/// 常量配置模块
mod const_config;
/// 领域层（纯业务逻辑）
pub(crate) mod domain;
/// 错误类型模块
mod error;
/// 基础设施层（TCP/事件循环/指标/限流等具体实现）
pub(crate) mod infrastructure;

pub extern crate bytes;
pub extern crate tokio;
pub extern crate tracing;
pub extern crate tracing_subscriber;

/// Re-export common error types
pub use error::{LynnError, Result};

/// Metrics module for monitoring
#[cfg(feature = "metrics")]
pub mod lynn_metrics {
    /// Re-export metrics collection
    pub use super::infrastructure::metrics::metrics::{METRICS, Metrics, Timer, export_metrics};
    /// Re-export metrics server
    pub use super::infrastructure::metrics::metrics_server::{
        MetricsServerConfig, serve_metrics, spawn_metrics_server,
    };
    /// Re-export prometheus for advanced usage
    pub use prometheus;
}

/// The server module, containing the server configuration API and server implementation.
#[cfg(feature = "server")]
pub mod lynn_server {
    /// The server implementation, handling incoming connections and requests.
    pub use super::application::server::lynn_server::LynnServer;
    /// The server configuration API, providing methods to configure the server.
    pub use super::application::server::server_config::LynnServerConfig;
    /// The server configuration builder, providing a fluent interface to build server configurations.
    pub use super::application::server::server_config::LynnServerConfigBuilder;
    /// The `ClientsContext` struct is used to manage the state and context of connected clients.
    pub use super::domain::handler::handler_system::ClientsContext;
}

/// The TCP dependents module, containing common types used by both the server and the client.
#[cfg(any(feature = "server", feature = "client"))]
pub mod lynn_tcp_dependents {
    /// The handler result type, used to represent the result of a request handler.
    pub use super::domain::model::handler_result::HandlerResult;
    /// The input buffer value object, representing the input data received by the server or client.
    pub use super::domain::model::input_buf_vo::InputBufVO;
    /// The input buffer value object trait, defining the behavior of input buffer value objects.
    pub use super::domain::model::input_buf_vo::InputBufVOTrait;
    /// The global state extractor (`AppState<T>`), injected into server handler parameters.
    pub use super::domain::state::app_state::AppState;
    /// Re-export of the state module for convenience.
    pub use super::lynn_state;
}

/// The global state module: shared values injected into server handler
/// parameters through `AppState<T>` (like axum's `State<T>`).
#[cfg(any(feature = "server", feature = "client"))]
pub mod lynn_state {
    /// The global state extractor (`AppState<T>`), injected into server handler parameters.
    pub use super::domain::state::app_state::AppState;
    /// The type-keyed registry holding the shared state values of a server.
    pub use super::domain::state::state_registry::StateRegistry;
}

/// Built-in SeaORM integration: register a database handle with
/// `LynnServer::with_db(...)` (or `with_state`) and extract it in handlers
/// through the [`DbConn`] alias. Requires the optional `seaorm` feature.
#[cfg(feature = "seaorm")]
pub mod lynn_seaorm {
    /// Re-export of `sea_orm` for convenient access in handler code.
    pub use sea_orm;
    /// Alias of `AppState<sea_orm::DatabaseConnection>` for handler parameters.
    pub type DbConn = super::domain::state::app_state::AppState<sea_orm::DatabaseConnection>;
}

/// The client module, containing the client configuration and client implementation.
#[cfg(feature = "client")]
pub mod lynn_client {
    /// The client configuration API, providing methods to configure the client.
    pub use super::application::client::client_config::LynnClientConfig;
    /// The client configuration builder, providing a fluent interface to build client configurations.
    pub use super::application::client::client_config::LynnClientConfigBuilder;
    /// The client implementation, handling outgoing connections and requests.
    pub use super::application::client::lynn_client::LynnClient;
}
