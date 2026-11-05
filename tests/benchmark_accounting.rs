#![cfg(all(feature = "client", feature = "server"))]

#[path = "../benches/support/model1.rs"]
mod model1;

use std::time::{Duration, Instant};

use lynn_tcp::{
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_tcp_dependents::HandlerResult,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    time::{sleep, timeout},
};

#[tokio::test]
async fn benchmark_counts_missing_replies_and_rejects_corrupt_payloads() {
    for reply in [None, Some(b'a'), Some(b'b')] {
        timeout(Duration::from_secs(3), async {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let config = LynnClientConfigBuilder::new()
                .with_server_addr(listener.local_addr().unwrap())
                .unwrap()
                .with_reconnect_max_attempts(&1)
                .build();
            let client = LynnClient::new_with_config(config).await.start().await;
            assert!(client.is_connected());
            let server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                loop {
                    let mut header = [0; 10];
                    socket.read_exact(&mut header).await.unwrap();
                    let size = u64::from_le_bytes(header[2..].try_into().unwrap());
                    assert!(size <= 1024);
                    let mut body = vec![0; size as usize];
                    socket.read_exact(&mut body).await.unwrap();
                    if body[0] != 1 {
                        continue;
                    }
                    assert_eq!(&body[3..body.len() - 2], b"a");
                    // One late reply: no second request can enter the measurement window.
                    sleep(Duration::from_millis(50)).await;
                    if let Some(byte) = reply {
                        body[3] = byte;
                        socket.write_all(&header).await.unwrap();
                        socket.write_all(&body).await.unwrap();
                    }
                    socket.read_to_end(&mut Vec::new()).await.unwrap();
                    break;
                }
            });
            let request = HandlerResult::new_with_send_to_server(1, vec![b'a'].into()).unwrap();
            let result = tokio::spawn(async move {
                let now = Instant::now();
                model1::model1_client(client, request, 1, now, now + Duration::from_millis(20))
                    .await
            })
            .await
            .unwrap();
            assert_eq!(result.requests, 1);
            assert_eq!(result.responses, u64::from(reply == Some(b'a')));
            assert_eq!(result.rtts_ns.len(), result.responses as usize);
            assert!(!result.failure_in_warmup);
            assert_eq!(
                result.exit,
                match reply {
                    None => "receive_timeout",
                    Some(b'a') => "window_complete",
                    _ => "payload_mismatch",
                }
            );
            server.await.unwrap();
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn benchmark_warmup_disconnect_is_failure_even_with_zero_measured_counts() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(listener.local_addr().unwrap())
        .unwrap()
        .with_reconnect_max_attempts(&1)
        .build();
    let client = LynnClient::new_with_config(config).await.start().await;
    assert!(client.is_connected());
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        socket.read_u8().await.unwrap();
    });
    let now = Instant::now();
    let result = timeout(
        Duration::from_secs(3),
        model1::model1_client(
            client,
            HandlerResult::new_with_send_to_server(1, vec![b'a'].into()).unwrap(),
            1,
            now + Duration::from_secs(1),
            now + Duration::from_millis(1020),
        ),
    )
    .await
    .unwrap();
    assert_eq!((result.requests, result.responses), (0, 0));
    assert!(result.failure_in_warmup);
    assert_ne!(result.exit, "window_complete");
    assert!(result.rtts_ns.is_empty());
    server.await.unwrap();
}
