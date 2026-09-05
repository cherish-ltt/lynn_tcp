//! Optional TLS 1.3 transport security (feature = `tls`).
//!
//! Built on `rustls` + `tokio-rustls` with the `ring` crypto provider and
//! restricted to TLS 1.3 only. The feature is disabled by default: servers
//! and clients opt in by configuring certificates through
//! [`TlsServerConfig`] / [`TlsClientConfig`].

pub(crate) mod tls_config;
pub(crate) mod tls_provider;
