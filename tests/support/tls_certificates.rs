use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

pub struct Certificates {
    directory: PathBuf,
    pub ca: String,
    pub server_cert: String,
    pub server_leaf: String,
    pub server_key: String,
    pub expired_cert: String,
    pub client_cert: String,
    pub client_leaf: String,
    pub client_key: String,
}

impl Certificates {
    pub fn new() -> Self {
        Self::generate(false)
    }

    pub fn with_intermediate() -> Self {
        Self::generate(true)
    }

    fn generate(with_intermediate: bool) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let directory = std::env::temp_dir().join(format!(
            "lynn-kcp-tls-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        let mut ca = CertificateParams::new(vec![]).unwrap();
        ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca.distinguished_name.push(DnType::CommonName, "test root");
        ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        let key = KeyPair::generate().unwrap();
        let certificate = ca.self_signed(&key).unwrap();
        let issuer = Issuer::new(ca, key);
        let (issuer, intermediate) = if with_intermediate {
            let mut params = CertificateParams::new(vec![]).unwrap();
            params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
            params
                .distinguished_name
                .push(DnType::CommonName, "test intermediate");
            params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
            let key = KeyPair::generate().unwrap();
            let certificate = params.signed_by(&key, &issuer).unwrap();
            (Issuer::new(params, key), certificate.pem())
        } else {
            (issuer, String::new())
        };
        let mut server =
            CertificateParams::new(vec!["localhost".into(), "127.0.0.1".into(), "::1".into()])
                .unwrap();
        server.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_key = KeyPair::generate().unwrap();
        let valid = server.signed_by(&server_key, &issuer).unwrap();
        server.not_before = rcgen::date_time_ymd(2000, 1, 1);
        server.not_after = rcgen::date_time_ymd(2001, 1, 1);
        let expired = server.signed_by(&server_key, &issuer).unwrap();
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
            server_cert: write("server.pem", format!("{}{intermediate}", valid.pem())),
            server_leaf: write("server-leaf.pem", valid.pem()),
            expired_cert: write("expired.pem", format!("{}{intermediate}", expired.pem())),
            server_key: write("server.key", server_key.serialize_pem()),
            client_cert: write("client.pem", format!("{}{intermediate}", client.pem())),
            client_leaf: write("client-leaf.pem", client.pem()),
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
