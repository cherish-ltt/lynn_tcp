#![cfg(all(feature = "client", feature = "tls"))]

fn code_only(source: &str) -> String {
    source
        .lines()
        .map(|line| line.split("//").next().unwrap())
        .flat_map(str::chars)
        .filter(|character| !character.is_whitespace())
        .collect()
}

#[test]
fn both_readme_tls_client_examples_match_compiled_configuration() {
    macro_rules! compiled_example {
        ($name:ident; $($code:tt)*) => {{
            $($code)*
            let _ = $name;
            stringify!($($code)*)
        }};
    }
    let compiled = (|| -> lynn_tcp::Result<&'static str> {
        Ok(compiled_example! { config;
            use lynn_tcp::{lynn_client::*, lynn_tls::*};
            let config = LynnClientConfigBuilder::new()
                .with_server_addr("127.0.0.1:9177")?
                .with_tls(TlsClientConfigBuilder::new()
                    .with_ca_cert_path("ca.pem")
                    .with_server_name("localhost")
                    .build(),
                )
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
            .find(|code| code.contains("use lynn_tcp::{lynn_client::*, lynn_tls::*};"))
            .expect("README must contain the TLS client example");
        assert_eq!(code_only(snippet), code_only(compiled));
    }
}
