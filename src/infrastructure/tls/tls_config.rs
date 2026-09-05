//! Public TLS configuration types for the server and the client.

/// Server-side TLS configuration.
///
/// TLS stays disabled unless an instance of this struct is attached to the
/// server configuration (see `LynnServerConfigBuilder::with_tls`). The
/// certificate chain and the private key are PEM encoded files.
#[derive(Debug, Clone)]
pub struct TlsServerConfig {
    /// Path to the PEM encoded certificate chain (leaf first).
    pub cert_path: String,
    /// Path to the PEM encoded private key matching the leaf certificate.
    pub key_path: String,
    /// Optional path to a PEM encoded CA used to verify client certificates.
    /// When set, mutual TLS is enforced: clients must present a certificate
    /// signed by this CA.
    pub client_ca_path: Option<String>,
}

impl TlsServerConfig {
    /// Creates a server TLS configuration for the given PEM files.
    pub fn new(cert_path: impl Into<String>, key_path: impl Into<String>) -> Self {
        Self {
            cert_path: cert_path.into(),
            key_path: key_path.into(),
            client_ca_path: None,
        }
    }

    /// Enables mutual TLS: clients must present a certificate signed by the
    /// CA stored in `client_ca_path`.
    pub fn with_client_ca(mut self, client_ca_path: impl Into<String>) -> Self {
        self.client_ca_path = Some(client_ca_path.into());
        self
    }
}

/// Client-side TLS configuration.
///
/// By default the client verifies the server certificate against the CA
/// file supplied through [`TlsClientConfigBuilder::with_ca_cert_path`].
#[derive(Debug, Clone, Default)]
pub struct TlsClientConfig {
    /// Path to the PEM encoded CA certificate(s) used to verify the server.
    pub ca_cert_path: Option<String>,
    /// Optional SNI name to verify the server against. Defaults to the IP
    /// literal of the server address.
    pub server_name: Option<String>,
    /// Optional PEM client certificate chain for mutual TLS.
    pub client_cert_path: Option<String>,
    /// Optional PEM client private key for mutual TLS.
    pub client_key_path: Option<String>,
    /// DANGEROUS: skip server certificate verification entirely. Only for
    /// local development and tests.
    pub danger_accept_invalid_certs: bool,
}

/// Builder for [`TlsClientConfig`].
#[derive(Debug, Clone, Default)]
pub struct TlsClientConfigBuilder {
    config: TlsClientConfig,
}

impl TlsClientConfigBuilder {
    /// Creates a builder with an empty (verification-less default) config.
    pub fn new() -> Self {
        Self::default()
    }

    /// Uses the PEM CA file at `path` to verify the server certificate.
    pub fn with_ca_cert_path(mut self, path: impl Into<String>) -> Self {
        self.config.ca_cert_path = Some(path.into());
        self
    }

    /// Overrides the SNI/server name used for verification. Defaults to the
    /// IP literal of the server address.
    pub fn with_server_name(mut self, name: impl Into<String>) -> Self {
        self.config.server_name = Some(name.into());
        self
    }

    /// Presents a client certificate (mutual TLS).
    pub fn with_client_auth(
        mut self,
        cert_path: impl Into<String>,
        key_path: impl Into<String>,
    ) -> Self {
        self.config.client_cert_path = Some(cert_path.into());
        self.config.client_key_path = Some(key_path.into());
        self
    }

    /// DANGEROUS: disables server certificate verification. Only use in
    /// trusted development environments.
    pub fn with_danger_accept_invalid_certs(mut self) -> Self {
        self.config.danger_accept_invalid_certs = true;
        self
    }

    /// Builds the client TLS configuration.
    pub fn build(self) -> TlsClientConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_config_defaults_have_no_client_ca() {
        let cfg = TlsServerConfig::new("cert.pem", "key.pem");
        assert_eq!(cfg.cert_path, "cert.pem");
        assert_eq!(cfg.key_path, "key.pem");
        assert!(cfg.client_ca_path.is_none());
    }

    #[test]
    fn server_config_client_ca_enables_mutual_tls() {
        let cfg = TlsServerConfig::new("cert.pem", "key.pem").with_client_ca("ca.pem");
        assert_eq!(cfg.client_ca_path.as_deref(), Some("ca.pem"));
    }

    #[test]
    fn client_builder_sets_every_field() {
        let cfg = TlsClientConfigBuilder::new()
            .with_ca_cert_path("ca.pem")
            .with_server_name("example.test")
            .with_client_auth("client.pem", "client.key")
            .build();

        assert_eq!(cfg.ca_cert_path.as_deref(), Some("ca.pem"));
        assert_eq!(cfg.server_name.as_deref(), Some("example.test"));
        assert_eq!(cfg.client_cert_path.as_deref(), Some("client.pem"));
        assert_eq!(cfg.client_key_path.as_deref(), Some("client.key"));
        assert!(!cfg.danger_accept_invalid_certs);
    }

    #[test]
    fn client_builder_danger_mode() {
        let cfg = TlsClientConfigBuilder::new()
            .with_danger_accept_invalid_certs()
            .build();
        assert!(cfg.danger_accept_invalid_certs);
        assert!(cfg.ca_cert_path.is_none());
    }

    #[test]
    fn client_config_default_is_secure() {
        let cfg = TlsClientConfig::default();
        assert!(cfg.ca_cert_path.is_none());
        assert!(!cfg.danger_accept_invalid_certs);
    }
}
