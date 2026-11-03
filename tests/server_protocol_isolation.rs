#![cfg(feature = "server")]

use std::{net::SocketAddr, time::Duration};

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    task::JoinHandle,
    time::{sleep, timeout},
};

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()]).unwrap()
}

async fn start(
    header: &'static u16,
    tail: &'static u16,
) -> (SocketAddr, JoinHandle<lynn_tcp::Result<()>>) {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_message_header_mark(header)
        .with_message_tail_mark(tail)
        .build();
    let task = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo)
            .start(),
    );
    timeout(Duration::from_secs(2), async {
        while TcpStream::connect(addr).await.is_err() {
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    (addr, task)
}

async fn roundtrip(addr: SocketAddr, header: u16, tail: u16) {
    let mut socket = TcpStream::connect(addr).await.unwrap();
    let mut frame = header.to_le_bytes().to_vec();
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.extend_from_slice(&[1, 1, 0]);
    frame.extend_from_slice(&tail.to_le_bytes());
    socket.write_all(&frame).await.unwrap();
    let mut response = [0; 17];
    timeout(Duration::from_secs(2), socket.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response[13..15], b"ok");
    assert_eq!(
        u16::from_le_bytes(response[..2].try_into().unwrap()),
        header
    );
    assert_eq!(u16::from_le_bytes(response[15..].try_into().unwrap()), tail);
}

#[tokio::test]
async fn independent_servers_keep_their_response_marks() {
    let (first, first_task) = start(&9177, &7719).await;
    roundtrip(first, 9177, 7719).await;
    let (second, second_task) = start(&0xABCD, &0xEF01).await;
    roundtrip(second, 0xABCD, 0xEF01).await;
    roundtrip(first, 9177, 7719).await;
    first_task.abort();
    second_task.abort();
    first_task.await.unwrap_err();
    second_task.await.unwrap_err();
    let (third, third_task) = start(&0x1234, &0x5678).await;
    roundtrip(third, 0x1234, 0x5678).await;
    third_task.abort();
    third_task.await.unwrap_err();
}
