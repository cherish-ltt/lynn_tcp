use super::*;
use crate::infrastructure::tls::tls_config::TlsClientConfigBuilder;
use rcgen::{BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair};
use std::{path::PathBuf, time::Duration};

struct Certificates {
    directory: PathBuf,
    ca: String,
    server_cert: String,
    server_key: String,
    client_cert: String,
    client_key: String,
}

impl Certificates {
    fn new() -> Self {
        let directory =
            std::env::temp_dir().join(format!("lynn-mtls-identity-{}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        let mut ca = CertificateParams::new(vec![]).unwrap();
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        let key = KeyPair::generate().unwrap();
        let certificate = ca.self_signed(&key).unwrap();
        let issuer = Issuer::new(ca, key);
        let mut server = CertificateParams::new(vec!["localhost".into()]).unwrap();
        server.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_key = KeyPair::generate().unwrap();
        let server = server.signed_by(&server_key, &issuer).unwrap();
        let mut client = CertificateParams::new(vec!["client.local".into()]).unwrap();
        client.extended_key_usages = vec![ExtendedKeyUsagePurpose::ClientAuth];
        let client_key = KeyPair::generate().unwrap();
        let client = client.signed_by(&client_key, &issuer).unwrap();
        let write = |name: &str, content: String| {
            let path = directory.join(name);
            std::fs::write(&path, content).unwrap();
            path.to_string_lossy().into_owned()
        };
        Self {
            ca: write("ca.pem", certificate.pem()),
            server_cert: write("server.pem", server.pem()),
            server_key: write("server.key", server_key.serialize_pem()),
            client_cert: write("client.pem", client.pem()),
            client_key: write("client.key", client_key.serialize_pem()),
            directory,
        }
    }
}

impl Drop for Certificates {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.directory).unwrap();
    }
}

#[tokio::test]
async fn verifier_modes_preserve_explicit_identity_and_mtls_enforcement() {
    let certs = Certificates::new();
    let server = build_server_acceptor(
        &TlsServerConfig::new(&certs.server_cert, &certs.server_key).with_client_ca(&certs.ca),
    )
    .unwrap();
    for danger in [true, false] {
        for identity in [false, true] {
            let mut config = TlsClientConfigBuilder::new()
                .with_ca_cert_path(&certs.ca)
                .with_server_name("localhost")
                .build();
            config.danger_accept_invalid_certs = danger;
            if identity {
                config.client_cert_path = Some(certs.client_cert.clone());
                config.client_key_path = Some(certs.client_key.clone());
            }
            let client = build_client_tls(&config, "127.0.0.1:9177").unwrap();
            let (client_io, server_io) = tokio::io::duplex(65536);
            let (_, accepted) = tokio::time::timeout(Duration::from_secs(2), async {
                tokio::join!(
                    client.connector.connect(client.server_name, client_io),
                    server.accept(server_io)
                )
            })
            .await
            .unwrap();
            assert_eq!(
                accepted.is_ok(),
                identity,
                "danger={danger}, identity={identity}: {:?}",
                accepted.err()
            );
        }
        let mut config = TlsClientConfigBuilder::new()
            .with_ca_cert_path(&certs.ca)
            .with_server_name("localhost")
            .with_client_auth(&certs.client_cert, &certs.client_key)
            .build();
        config.danger_accept_invalid_certs = danger;
        for partial in [true, false] {
            let mut invalid = config.clone();
            if partial {
                invalid.client_key_path = None;
            } else {
                invalid.client_cert_path = None;
            }
            assert!(build_client_tls(&invalid, "127.0.0.1:9177").is_err());
        }
        config.client_key_path = Some(certs.server_key.clone());
        assert!(
            build_client_tls(&config, "127.0.0.1:9177").is_err(),
            "mismatched key must fail"
        );
        config.client_key_path = Some(
            certs
                .directory
                .join("missing.key")
                .to_string_lossy()
                .into_owned(),
        );
        assert!(
            build_client_tls(&config, "127.0.0.1:9177").is_err(),
            "missing key must fail"
        );
        config.client_key_path = Some(certs.client_key.clone());
        config.client_cert_path = Some(certs.client_key.clone());
        assert!(
            build_client_tls(&config, "127.0.0.1:9177").is_err(),
            "invalid certificate file must fail"
        );
    }
}
