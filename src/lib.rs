#![allow(unused)]
#![allow(private_interfaces)]
#![allow(private_bounds)]
#![allow(deprecated)]
#![allow(non_snake_case)]

/// The application module, containing the server configuration API and server implementation.
mod app;
/// The client module, containing the client configuration and client implementation.
mod client;
/// The constant configuration module, containing constants used throughout the application.
mod const_config;
/// The DTO factory module, responsible for creating data transfer objects.
mod dto_factory;
mod handler;
mod macros;
/// The service module, containing the application's business logic.
mod service;
/// The VO factory module, responsible for creating value objects.
mod vo_factory;

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
