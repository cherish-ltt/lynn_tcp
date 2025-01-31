#![allow(unused)]
#![allow(private_interfaces)]
#![allow(private_bounds)]
#![allow(deprecated)]
#![allow(non_snake_case)]
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
//! ```rust
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
//! ```rust
//! use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let _ = LynnServer::new_with_config(
//!         LynnServerConfigBuilder::new()
//!             .with_server_ipv4("0.0.0.0:9177")
//!             .with_server_max_connections(Some(&200))
//!             .with_server_max_threadpool_size(&10)
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
//! ```rust
//! use lynn_tcp::{
//!     lynn_client::LynnClient,
//!     lynn_tcp_dependents::*,
//! };
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let client = LynnClient::new_with_ipv4("127.0.0.1:9177")
//!             .await
//!             .start()
//!             .await;
//!     let _ = client.send_data(HandlerResult::new_with_send_to_server(1, "hello".into())).await;
//!     let input_buf_vo = client.get_receive_data().await.unwrap();
//!     Ok(())
//! }
//! ```

/// The application module, containing the server configuration API and server implementation.
mod app;
/// The client module, containing the client configuration and client implementation.
mod client;
/// The constant configuration module, containing constants used throughout the application.
mod const_config;
/// The DTO factory module, responsible for creating data transfer objects.
mod dto_factory;
/// The handler module, containing the implementation of request handlers.
mod handler;
/// The macros module, containing custom macros used throughout the application.
mod macros;
/// The VO factory module, responsible for creating value objects.
mod vo_factory;

pub extern crate bytes;
pub extern crate tokio;
pub extern crate tracing;
pub extern crate tracing_subscriber;

/// The server module, containing the server configuration API and server implementation.
#[cfg(feature = "server")]
pub mod lynn_server {
    /// The server configuration API, providing methods to configure the server.
    pub use super::app::lynn_config_api::LynnServerConfig;
    /// The server configuration builder, providing a fluent interface to build server configurations.
    pub use super::app::lynn_config_api::LynnServerConfigBuilder;
    /// The server implementation, handling incoming connections and requests.
    pub use super::app::LynnServer;
    /// The `ClientsContext` struct is used to manage the state and context of connected clients.
    pub use super::handler::ClientsContext;
}

/// The TCP dependents module, containing common types used by both the server and client.
#[cfg(any(feature = "server", feature = "client"))]
pub mod lynn_tcp_dependents {
    /// The handler result type, used to represent the result of a request handler.
    pub use super::dto_factory::input_dto::HandlerResult;
    /// The input buffer value object, representing the input data received by the server or client.
    pub use super::vo_factory::input_vo::InputBufVO;
    /// The input buffer value object trait, defining the behavior of input buffer value objects.
    pub use super::vo_factory::InputBufVOTrait;
}

/// The client module, containing the client configuration and client implementation.
#[cfg(feature = "client")]
pub mod lynn_client {
    /// The client configuration API, providing methods to configure the client.
    pub use super::client::client_config::LynnClientConfig;
    /// The client configuration builder, providing a fluent interface to build client configurations.
    pub use super::client::client_config::LynnClientConfigBuilder;
    /// The client implementation, handling outgoing connections and requests.
    pub use super::client::LynnClient;
}
