#![cfg(feature = "server")]

use std::time::Duration;

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{sleep, timeout},
};

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()])
}

async fn start(read_timeout: &'static u64) -> std::net::SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_connection_rate_limit(&0)
        .with_max_connections_per_ip(&1)
        .with_server_check_heart_timeout_time(&60)
        .with_read_timeout_secs(read_timeout)
        .build();
    tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo)
            .start(),
    );
    sleep(Duration::from_millis(100)).await;
    addr
}

fn frame() -> Vec<u8> {
    let mut frame = Vec::from(9177u16.to_le_bytes());
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.extend_from_slice(&[1, 1, 0]);
    frame.extend_from_slice(&7719u16.to_le_bytes());
    frame
}

#[tokio::test]
async fn read_timeout_closes_idle_connection_and_reclaims_quota() {
    let addr = start(&1).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    assert_eq!(
        timeout(Duration::from_millis(1800), socket.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    let mut replacement = TcpStream::connect(addr).await.unwrap();
    replacement.write_all(&frame()).await.unwrap();
    let mut response = [0; 17];
    timeout(
        Duration::from_secs(1),
        replacement.read_exact(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(&response[13..15], b"ok");
}

#[tokio::test]
async fn partial_frame_without_progress_times_out() {
    let addr = start(&1).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(&frame()[..11]).await.unwrap();
    assert_eq!(
        timeout(Duration::from_millis(1800), socket.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn read_progress_renews_idle_deadline() {
    let addr = start(&1).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    let frame = frame();
    for chunk in frame.chunks(5) {
        socket.write_all(chunk).await.unwrap();
        sleep(Duration::from_millis(600)).await;
    }
    let mut response = [0; 17];
    timeout(Duration::from_secs(1), socket.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response[13..15], b"ok");
}

#[tokio::test]
async fn zero_read_timeout_keeps_idle_connection_usable() {
    let addr = start(&0).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    sleep(Duration::from_millis(1300)).await;
    socket.write_all(&frame()).await.unwrap();
    let mut response = [0; 17];
    timeout(Duration::from_secs(1), socket.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response[13..15], b"ok");
}
