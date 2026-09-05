//! Error types for lynn_tcp framework
//!
//! This module defines comprehensive error types using `thiserror` for better error handling.

use std::io;
use thiserror::Error;

/// Main error type for lynn_tcp framework
#[derive(Error, Debug)]
pub enum LynnError {
    /// IO errors
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    /// Network-related errors
    #[error("Network error: {0}")]
    Network(String),

    /// Connection errors
    #[error("Connection error: {0}")]
    Connection(String),

    /// Address parsing errors
    #[error("Invalid address: {0}")]
    InvalidAddress(String),

    /// Configuration errors
    #[error("Configuration error: {0}")]
    Config(String),

    /// Buffer operation errors
    #[error("Buffer error: {0}")]
    Buffer(String),

    /// Message protocol errors
    #[error("Protocol error: {0}")]
    Protocol(String),

    /// Handler errors
    #[error("Handler error: {0}")]
    Handler(String),

    /// Timeout errors
    #[error("Timeout: {0}")]
    Timeout(String),

    /// Errors related to client operations
    #[error("Client error: {0}")]
    Client(String),

    /// Errors related to server operations
    #[error("Server error: {0}")]
    Server(String),

    /// TLS handshake/configuration errors (feature `tls`)
    #[error("TLS error: {0}")]
    Tls(String),

    /// Generic errors
    #[error("Error: {0}")]
    Generic(String),
}

/// Result type alias for lynn_tcp
pub type Result<T> = std::result::Result<T, LynnError>;

impl LynnError {
    /// Create a network error
    pub fn network<S: Into<String>>(msg: S) -> Self {
        Self::Network(msg.into())
    }

    /// Create a connection error
    pub fn connection<S: Into<String>>(msg: S) -> Self {
        Self::Connection(msg.into())
    }

    /// Create an invalid address error
    pub fn invalid_address<S: Into<String>>(msg: S) -> Self {
        Self::InvalidAddress(msg.into())
    }

    /// Create a configuration error
    pub fn config<S: Into<String>>(msg: S) -> Self {
        Self::Config(msg.into())
    }

    /// Create a buffer error
    pub fn buffer<S: Into<String>>(msg: S) -> Self {
        Self::Buffer(msg.into())
    }

    /// Create a protocol error
    pub fn protocol<S: Into<String>>(msg: S) -> Self {
        Self::Protocol(msg.into())
    }

    /// Create a handler error
    pub fn handler<S: Into<String>>(msg: S) -> Self {
        Self::Handler(msg.into())
    }

    /// Create a timeout error
    pub fn timeout<S: Into<String>>(msg: S) -> Self {
        Self::Timeout(msg.into())
    }

    /// Create a client error
    pub fn client<S: Into<String>>(msg: S) -> Self {
        Self::Client(msg.into())
    }

    /// Create a server error
    pub fn server<S: Into<String>>(msg: S) -> Self {
        Self::Server(msg.into())
    }

    /// Create a TLS error
    pub fn tls<S: Into<String>>(msg: S) -> Self {
        Self::Tls(msg.into())
    }
}

// Helper functions for common error conversions
pub trait ToLynnError<T> {
    fn with_context<S: Into<String>>(self, ctx: S) -> Result<T>;
}

impl<T, E> ToLynnError<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn with_context<S: Into<String>>(self, ctx: S) -> Result<T> {
        self.map_err(|e| LynnError::Generic(format!("{}: {}", ctx.into(), e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io;

    #[test]
    fn constructors_produce_expected_variants() {
        assert!(matches!(LynnError::network("n"), LynnError::Network(_)));
        assert!(matches!(
            LynnError::connection("c"),
            LynnError::Connection(_)
        ));
        assert!(matches!(
            LynnError::invalid_address("a"),
            LynnError::InvalidAddress(_)
        ));
        assert!(matches!(LynnError::config("c"), LynnError::Config(_)));
        assert!(matches!(LynnError::buffer("b"), LynnError::Buffer(_)));
        assert!(matches!(LynnError::protocol("p"), LynnError::Protocol(_)));
        assert!(matches!(LynnError::handler("h"), LynnError::Handler(_)));
        assert!(matches!(LynnError::timeout("t"), LynnError::Timeout(_)));
        assert!(matches!(LynnError::client("c"), LynnError::Client(_)));
        assert!(matches!(LynnError::server("s"), LynnError::Server(_)));
    }

    #[test]
    fn display_is_human_readable() {
        assert_eq!(
            LynnError::Timeout("too long".into()).to_string(),
            "Timeout: too long"
        );
        assert_eq!(
            LynnError::InvalidAddress("x".into()).to_string(),
            "Invalid address: x"
        );
    }

    #[test]
    fn io_errors_convert_automatically() {
        let e: LynnError = io::Error::other("disk").into();
        assert!(matches!(e, LynnError::Io(_)));
        assert_eq!(e.to_string(), "IO error: disk");
    }

    #[test]
    fn with_context_passes_success_through() {
        let ok: std::result::Result<i32, io::Error> = Ok(5);
        assert_eq!(ok.with_context("ctx").unwrap(), 5);
    }

    #[test]
    fn with_context_wraps_failures() {
        let err: std::result::Result<i32, io::Error> = Err(io::Error::other("boom"));
        let wrapped = err.with_context("reading config").unwrap_err();
        assert!(matches!(wrapped, LynnError::Generic(_)));
        assert_eq!(wrapped.to_string(), "Error: reading config: boom");
    }
}
