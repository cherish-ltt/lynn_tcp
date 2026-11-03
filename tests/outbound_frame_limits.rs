#![cfg(all(feature = "server", feature = "client"))]

use std::time::Duration;

use lynn_tcp::{
    LynnError,
    bytes::Bytes,
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_server::{ClientsContext, LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
};
use tokio::{
    net::TcpStream,
    time::{sleep, timeout},
};

const OVERSIZED_PAYLOAD: usize = 10 * 1024 * 1024 - 4;

async fn reject_oversized_broadcast(clients: ClientsContext) -> HandlerResult {
    let addrs = clients.get_all_clients_addrs().await;
    let result =
        HandlerResult::new_with_send(1, Bytes::from(vec![0; OVERSIZED_PAYLOAD]), addrs.clone());
    assert!(matches!(result, Err(LynnError::Protocol(_))));
    HandlerResult::new_with_send(1, "rejected".into(), addrs).unwrap()
}

async fn echo(input: InputBufVO) -> HandlerResult {
    let addr = input.get_input_addr().unwrap();
    HandlerResult::new_with_send(2, addr.to_string().into(), vec![addr]).unwrap()
}

async fn receive(client: &mut LynnClient<'_>) -> Vec<u8> {
    let response = timeout(Duration::from_secs(2), client.get_receive_data())
        .await
        .unwrap()
        .unwrap();
    response.get_all_bytes().to_vec()
}

async fn check_connections(
    server_config: LynnServerConfigBuilder<'static>,
    client_configs: [LynnClientConfigBuilder<'static>; 2],
) {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = server_config
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .build();
    let server = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .add_router(1, reject_oversized_broadcast)
            .add_router(2, echo)
            .start(),
    );
    timeout(Duration::from_secs(2), async {
        while TcpStream::connect(addr).await.is_err() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let [first_config, second_config] =
        client_configs.map(|config| config.with_server_addr(addr).unwrap().build());
    let mut first = LynnClient::new_with_config(first_config)
        .await
        .start()
        .await;
    assert!(first.is_connected());
    let mut second = LynnClient::new_with_config(second_config)
        .await
        .start()
        .await;
    assert!(second.is_connected());
    first
        .send_data(HandlerResult::new_with_send_to_server(2, Bytes::new()).unwrap())
        .await
        .unwrap();
    let first_addr = receive(&mut first).await;
    second
        .send_data(HandlerResult::new_with_send_to_server(2, Bytes::new()).unwrap())
        .await
        .unwrap();
    let second_addr = receive(&mut second).await;

    assert!(matches!(
        HandlerResult::new_with_send_to_server(1, Bytes::from(vec![0; OVERSIZED_PAYLOAD])),
        Err(LynnError::Protocol(_))
    ));
    let sender = first.get_sender().await.unwrap();
    sender
        .send(HandlerResult::new_with_send_to_server(1, Bytes::new()).unwrap())
        .await
        .unwrap();
    assert_eq!(receive(&mut first).await, b"rejected");
    assert_eq!(receive(&mut second).await, b"rejected");
    for (client, addr) in [(&mut first, first_addr), (&mut second, second_addr)] {
        client
            .send_data(HandlerResult::new_with_send_to_server(2, Bytes::new()).unwrap())
            .await
            .unwrap();
        assert_eq!(
            receive(client).await,
            addr,
            "the connection must not be replaced"
        );
        assert!(client.is_connected());
    }
    server.abort();
    server.await.unwrap_err();
}

#[tokio::test]
async fn tcp_oversized_requests_and_broadcasts_preserve_connections() {
    check_connections(
        LynnServerConfigBuilder::new(),
        [
            LynnClientConfigBuilder::new(),
            LynnClientConfigBuilder::new(),
        ],
    )
    .await;
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn tls_oversized_requests_and_broadcasts_preserve_connections() {
    use lynn_tcp::lynn_tls::{TlsClientConfigBuilder, TlsServerConfig};
    let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let directory =
        std::env::temp_dir().join(format!("lynn-outbound-limits-{}", std::process::id()));
    std::fs::create_dir(&directory).unwrap();
    let cert = directory.join("cert.pem");
    let key = directory.join("key.pem");
    std::fs::write(&cert, certificate.cert.pem()).unwrap();
    std::fs::write(&key, certificate.signing_key.serialize_pem()).unwrap();
    check_connections(
        LynnServerConfigBuilder::new().with_tls(TlsServerConfig::new(
            cert.to_str().unwrap(),
            key.to_str().unwrap(),
        )),
        std::array::from_fn(|_| {
            LynnClientConfigBuilder::new().with_tls(
                TlsClientConfigBuilder::new()
                    .with_ca_cert_path(cert.to_str().unwrap())
                    .with_server_name("localhost")
                    .build(),
            )
        }),
    )
    .await;
    std::fs::remove_dir_all(directory).unwrap();
}
