use super::{Certificates, KcpConfig, KcpStream, Mode, tls_configs};
use crate::{
    domain::model::message_validation::MAX_MESSAGE_SIZE,
    infrastructure::{kcp::TLS_READY, tls::tls_provider::ClientTls},
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_state::AppState,
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
    lynn_tls::TlsServerConfig,
};
use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    time::timeout,
};

pub(super) async fn connect(
    addr: SocketAddr,
    client: &ClientTls,
) -> tokio_rustls::client::TlsStream<KcpStream> {
    timeout(Duration::from_secs(3), async {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
        socket.connect(addr).await.unwrap();
        let (stream, ready) =
            KcpStream::start(socket, addr, 42, KcpConfig::default(), Mode::Tls, None).unwrap();
        ready.await.unwrap().unwrap();
        let mut stream = client
            .connector
            .connect(client.server_name.clone(), stream)
            .await
            .unwrap();
        let mut ready = [0; TLS_READY.len()];
        stream.read_exact(&mut ready).await.unwrap();
        assert_eq!(ready, TLS_READY);
        stream
    })
    .await
    .expect("authenticated replacement must fit the server's single connection slot")
}

pub(super) fn frame(payload: &[u8]) -> Vec<u8> {
    let mut bytes = 9177u16.to_le_bytes().to_vec();
    bytes.extend_from_slice(&(payload.len() as u64 + 5).to_le_bytes());
    bytes.extend_from_slice(&[1, 1, 0]);
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&7719u16.to_le_bytes());
    bytes
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn split_coalesced_and_invalid_tls_frames_use_the_server_parser() {
    async fn echo(input: InputBufVO, calls: AppState<Arc<AtomicUsize>>) -> HandlerResult {
        calls.fetch_add(1, Ordering::SeqCst);
        HandlerResult::new_with_send(
            1,
            input.get_all_bytes().into(),
            vec![input.get_input_addr().unwrap()],
        )
        .unwrap()
    }
    let certs = Certificates::new();
    let (client, _) = tls_configs(&certs);
    let calls = Arc::new(AtomicUsize::new(0));
    let server = LynnServer::new_with_config(
        LynnServerConfigBuilder::new()
            .with_addr("127.0.0.1:0")
            .unwrap()
            .with_kcp(KcpConfig::default())
            .with_tls(TlsServerConfig::new(&certs.server_cert, &certs.server_key))
            .with_server_max_connections(Some(&1))
            .with_max_connections_per_ip(&1)
            .with_connection_rate_limit(&0)
            .with_server_max_taskpool_size(&1)
            .build(),
    )
    .await
    .with_state(calls.clone())
    .add_router(1, echo);
    let ready = server.startup_handle();
    let task = tokio::spawn(server.start());
    let addr = timeout(Duration::from_secs(3), ready.wait())
        .await
        .unwrap()
        .unwrap();
    let mut peer = connect(addr, &client).await;

    let split = frame(b"split");
    timeout(Duration::from_secs(3), async {
        let mut sent = 0;
        for end in [1, 5, 10, split.len() - 1] {
            peer.write_all(&split[sent..end]).await.unwrap();
            peer.flush().await.unwrap();
            sent = end;
        }
        assert!(
            timeout(Duration::from_millis(25), peer.read_u8())
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        peer.write_all(&split[sent..]).await.unwrap();
        peer.flush().await.unwrap();
        let mut response = vec![0; split.len()];
        peer.read_exact(&mut response).await.unwrap();
        assert_eq!(response, split);

        let mut expected = vec![frame(b"one"), frame(b"two")];
        let coalesced = expected.concat();
        peer.write_all(&coalesced).await.unwrap();
        peer.flush().await.unwrap();
        let mut response = vec![0; coalesced.len()];
        peer.read_exact(&mut response).await.unwrap();
        let mut received: Vec<_> = response
            .chunks_exact(expected[0].len())
            .map(|b| b.to_vec())
            .collect();
        received.sort();
        expected.sort();
        assert_eq!(received, expected);
    })
    .await
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 3);

    let mut invalid = frame(b"invalid");
    *invalid.last_mut().unwrap() ^= 1;
    let mut oversized = 9177u16.to_le_bytes().to_vec();
    oversized.extend_from_slice(&(MAX_MESSAGE_SIZE as u64 + 1).to_le_bytes());
    for mut rejected in [invalid, oversized] {
        let before = calls.load(Ordering::SeqCst);
        rejected.extend_from_slice(&frame(b"must not resynchronize"));
        timeout(Duration::from_secs(3), async {
            peer.write_all(&rejected).await.unwrap();
            peer.flush().await.unwrap();
        })
        .await
        .unwrap();
        // Keep the invalid peer alive: replacing it before the 60s idle limit proves server closure.
        let mut replacement = connect(addr, &client).await;
        assert_eq!(calls.load(Ordering::SeqCst), before);
        let expected = frame(b"available");
        let mut response = vec![0; expected.len()];
        timeout(Duration::from_secs(3), async {
            replacement.write_all(&expected).await.unwrap();
            replacement.flush().await.unwrap();
            replacement.read_exact(&mut response).await.unwrap();
        })
        .await
        .unwrap();
        assert_eq!(response, expected);
        assert_eq!(calls.load(Ordering::SeqCst), before + 1);
        peer = replacement;
    }
    drop(peer);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}
