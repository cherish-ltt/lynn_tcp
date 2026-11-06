#![cfg(all(feature = "server", feature = "client", feature = "kcp-tls"))]

fn code_only(source: &str) -> String {
    source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

#[test]
fn paired_kcp_tls_configuration_matches_both_readmes() {
    macro_rules! compiled_example {
        ($server:ident, $client:ident; $($code:tt)*) => {{
            $($code)*
            let _ = ($server, $client);
            stringify!($($code)*)
        }};
    }
    let compiled = (|| -> lynn_tcp::Result<&'static str> {
        Ok(compiled_example! { server, client;
            use lynn_tcp::{
                lynn_client::LynnClientConfigBuilder,
                lynn_kcp::KcpConfig,
                lynn_server::LynnServerConfigBuilder,
                lynn_tls::{TlsClientConfigBuilder, TlsServerConfig},
            };

            let server = LynnServerConfigBuilder::new()
                .with_addr("127.0.0.1:9177")?
                .with_kcp(KcpConfig::default())
                .with_tls(TlsServerConfig::new("server.pem", "server.key"))
                .build();
            let client = LynnClientConfigBuilder::new()
                .with_server_addr("127.0.0.1:9177")?
                .with_kcp(KcpConfig::default())
                .with_tls(TlsClientConfigBuilder::new().with_ca_cert_path("ca.pem").build())
                .build();
        })
    })()
    .unwrap();
    for readme in [
        include_str!("../README.md"),
        include_str!("../README_ZH.md"),
    ] {
        let snippet = readme
            .split("```rust\n")
            .filter_map(|block| block.split_once("```"))
            .map(|(code, _)| code)
            .find(|code| code.contains("lynn_tls::{TlsClientConfigBuilder, TlsServerConfig}"))
            .expect("README must contain the paired KCP+TLS configuration");
        assert_eq!(code_only(snippet), code_only(compiled));
    }
}
