#![allow(unused)]
#![allow(private_interfaces)]
#![allow(private_bounds)]

// Module containing application-specific components
mod app;
// Module containing constant configurations
mod const_config;
// Module containing data transfer objects and factory functions
mod dto_factory;
// Module containing service interfaces and implementations
mod service;
mod vo_factory;
mod client;

/// The main library module for the Lynn TCP Server.
///
/// This module exports various components and utilities for building and running a TCP server.
pub mod server {
    // Exports the LynnServerConfigBuilder for configuring the server
    pub use super::app::lynn_config_api::LynnServerConfigBuilder;
    // Exports the LynnServerConfig for configuring the server
    pub use super::app::lynn_config_api::LynnServerConfig;
    // Exports the LynnServer for running the server
    pub use super::app::LynnServer;
    // Exports the HandlerResult for handling the results of server operations
    pub use super::dto_factory::input_dto::HandlerResult;
    // Exports the InputBufVO for handling input data
    pub use super::vo_factory::input_vo::InputBufVO;
    pub use super::vo_factory::InputBufVOTrait;
}
