#![cfg(feature = "server")]

use std::time::Duration;

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{advance, sleep, timeout},
};

#[tokio::test]
async fn worker_responds_after_long_idle() {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&1)
        .with_max_connections_per_ip(&0)
        .with_connection_rate_limit(&0)
        .with_server_check_heart_timeout_time(&100_000)
        .build();
    async fn echo(input: InputBufVO) -> HandlerResult {
        HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()])
            .expect("payload fits the protocol limit")
    }
    let server = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo)
            .start(),
    );
    sleep(Duration::from_millis(100)).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    let mut frame = Vec::new();
    frame.extend_from_slice(&9177u16.to_le_bytes());
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.extend_from_slice(&[1, 1, 0]);
    frame.extend_from_slice(&7719u16.to_le_bytes());
    let mut response = [0; 17];
    socket.write_all(&frame).await.unwrap();
    timeout(Duration::from_secs(2), socket.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response[13..15], b"ok");

    tokio::time::pause();
    for _ in 0..70_000 {
        advance(Duration::from_millis(30)).await;
        tokio::task::yield_now().await;
    }
    tokio::time::resume();

    for _ in 0..2 {
        socket.write_all(&frame).await.unwrap();
        timeout(Duration::from_secs(2), socket.read_exact(&mut response))
            .await
            .expect("worker must survive long idle and reset its backoff")
            .unwrap();
        assert_eq!(&response[13..15], b"ok");
    }
    server.abort();
}
