//! rustls configuration builders (TLS 1.3 only, ring provider).

use std::{io::BufReader, sync::Arc};

use rustls::{
    RootCertStore,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{self, CryptoProvider, ring as ring_provider},
    pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime},
};
use tokio_rustls::{TlsAcceptor, TlsConnector};

use crate::{
    LynnError, Result,
    infrastructure::tls::tls_config::{TlsClientConfig, TlsServerConfig},
};

/// Loads PEM encoded certificates from `path`.
fn load_certs(path: &str) -> Result<Vec<CertificateDer<'static>>> {
    let file = std::fs::File::open(path)
        .map_err(|e| LynnError::tls(format!("failed to open certificate file {path}: {e}")))?;
    rustls_pemfile::certs(&mut BufReader::new(file))
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| LynnError::tls(format!("failed to parse certificates from {path}: {e}")))
}

/// Loads the first PEM encoded private key from `path`.
fn load_private_key(path: &str) -> Result<PrivateKeyDer<'static>> {
    let file = std::fs::File::open(path)
        .map_err(|e| LynnError::tls(format!("failed to open key file {path}: {e}")))?;
    rustls_pemfile::private_key(&mut BufReader::new(file))?
        .ok_or_else(|| LynnError::tls(format!("no private key found in {path}")))
}

/// Loads a root store populated from a PEM CA bundle file.
fn load_root_store(path: &str) -> Result<RootCertStore> {
    let certs = load_certs(path)?;
    let mut roots = RootCertStore::empty();
    for cert in certs {
        roots
            .add(cert)
            .map_err(|e| LynnError::tls(format!("failed to add CA from {path}: {e}")))?;
    }
    Ok(roots)
}

/// The process-wide crypto provider (ring), installed once.
fn provider() -> Arc<CryptoProvider> {
    Arc::new(ring_provider::default_provider())
}

/// Builds a TLS 1.3 only server acceptor from the given configuration.
pub(crate) fn build_server_acceptor(config: &TlsServerConfig) -> Result<TlsAcceptor> {
    let certs = load_certs(&config.cert_path)?;
    let key = load_private_key(&config.key_path)?;

    let builder = rustls::ServerConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| LynnError::tls(format!("failed to enable TLS 1.3: {e}")))?;

    let builder = match &config.client_ca_path {
        Some(ca_path) => {
            let roots = load_root_store(ca_path)?;
            let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
                .build()
                .map_err(|e| LynnError::tls(format!("failed to build client verifier: {e}")))?;
            builder.with_client_cert_verifier(verifier)
        },
        None => builder.with_no_client_auth(),
    };

    let server_config = builder
        .with_single_cert(certs, key)
        .map_err(|e| LynnError::tls(format!("invalid server certificate/key pair: {e}")))?;

    Ok(TlsAcceptor::from(Arc::new(server_config)))
}

/// A ready-to-use client TLS endpoint: connector plus the resolved server name.
#[derive(Clone)]
pub(crate) struct ClientTls {
    pub(crate) connector: TlsConnector,
    pub(crate) server_name: ServerName<'static>,
}

/// Builds a TLS 1.3 only client connector from the given configuration and
/// resolves the server name to verify against.
pub(crate) fn build_client_tls(config: &TlsClientConfig, fallback_addr: &str) -> Result<ClientTls> {
    let server_name = resolve_server_name(config, fallback_addr)?;

    let builder = rustls::ClientConfig::builder_with_provider(provider())
        .with_protocol_versions(&[&rustls::version::TLS13])
        .map_err(|e| LynnError::tls(format!("failed to enable TLS 1.3: {e}")))?;

    let client_config = if config.danger_accept_invalid_certs {
        tracing::warn!(
            "TLS server certificate verification is DISABLED (danger_accept_invalid_certs); only use in development"
        );
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoServerCertVerifier::new()))
            .with_no_client_auth()
    } else {
        let ca_path = config.ca_cert_path.as_deref().ok_or_else(|| {
            LynnError::tls(
                "no TLS trust anchor configured: set `ca_cert_path` (or enable `danger_accept_invalid_certs` for development)",
            )
        })?;
        let roots = load_root_store(ca_path)?;
        let builder = builder.with_root_certificates(roots);
        match (&config.client_cert_path, &config.client_key_path) {
            (Some(cert_path), Some(key_path)) => builder
                .with_client_auth_cert(load_certs(cert_path)?, load_private_key(key_path)?)
                .map_err(|e| LynnError::tls(format!("invalid client certificate/key pair: {e}")))?,
            _ => builder.with_no_client_auth(),
        }
    };

    Ok(ClientTls {
        connector: TlsConnector::from(Arc::new(client_config)),
        server_name,
    })
}

/// Resolves the server name used for SNI and certificate verification.
fn resolve_server_name(
    config: &TlsClientConfig,
    fallback_addr: &str,
) -> Result<ServerName<'static>> {
    let raw = match &config.server_name {
        Some(name) => name.clone(),
        // Fall back to the IP literal of the server address ("ip:port").
        None => fallback_addr
            .rsplit_once(':')
            .map(|(host, _)| host.to_string())
            .unwrap_or_else(|| fallback_addr.to_string()),
    };
    ServerName::try_from(raw.clone())
        .map_err(|e| LynnError::tls(format!("invalid TLS server name '{raw}': {e}")))
}

/// A `ServerCertVerifier` that accepts every server certificate.
///
/// Only used when the user explicitly opted into
/// `danger_accept_invalid_certs`.
#[derive(Debug)]
struct NoServerCertVerifier {
    supported_schemes: Vec<rustls::SignatureScheme>,
}

impl NoServerCertVerifier {
    fn new() -> Self {
        Self {
            supported_schemes: ring_provider::default_provider()
                .signature_verification_algorithms
                .supported_schemes(),
        }
    }
}

impl ServerCertVerifier for NoServerCertVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
        Ok(HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.supported_schemes.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::tls::tls_config::TlsClientConfigBuilder;

    fn expect_tls_err<T>(result: Result<T>) -> LynnError {
        match result {
            Err(e) => e,
            Ok(_) => panic!("expected a TLS configuration error"),
        }
    }

    #[test]
    fn server_acceptor_fails_fast_on_missing_cert() {
        let cfg = TlsServerConfig::new("/nonexistent/cert.pem", "/nonexistent/key.pem");
        let err = expect_tls_err(build_server_acceptor(&cfg));
        assert!(err.to_string().contains("cert.pem"), "got: {err}");
    }

    #[test]
    fn client_tls_requires_a_trust_anchor_by_default() {
        let cfg = TlsClientConfig::default();
        let err = expect_tls_err(build_client_tls(&cfg, "127.0.0.1:9177"));
        assert!(err.to_string().contains("trust anchor"), "got: {err}");
    }

    #[test]
    fn client_tls_danger_mode_needs_no_ca() {
        let cfg = TlsClientConfigBuilder::new()
            .with_danger_accept_invalid_certs()
            .build();
        let tls = build_client_tls(&cfg, "127.0.0.1:9177").expect("danger mode builds");
        assert_eq!(
            tls.server_name.to_str().as_ref(),
            "127.0.0.1",
            "server name defaults to the address host"
        );
    }

    #[test]
    fn server_name_override_wins() {
        let cfg = TlsClientConfigBuilder::new()
            .with_ca_cert_path("/nonexistent/ca.pem")
            .with_server_name("example.test")
            .build();
        // Fails later on the missing CA file, but the name must resolve first.
        let err = expect_tls_err(build_client_tls(&cfg, "127.0.0.1:9177"));
        assert!(
            err.to_string().contains("ca.pem"),
            "name resolution passed, CA load failed: {err}"
        );
    }

    #[test]
    fn invalid_server_name_is_rejected() {
        let cfg = TlsClientConfigBuilder::new()
            .with_danger_accept_invalid_certs()
            .with_server_name("")
            .build();
        let err = expect_tls_err(build_client_tls(&cfg, "127.0.0.1:9177"));
        assert!(err.to_string().contains("server name"), "got: {err}");
    }
}
