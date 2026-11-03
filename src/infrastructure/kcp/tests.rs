use std::{sync::Arc, time::Duration};

use kcp_ovo::{KcpCmd, core::kcp::Kcp, helper::KcpPacketHeader};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    sync::mpsc,
    time::{sleep, timeout},
};

use super::{HELLO, KcpConfig, PACKETS, stream::KcpStream};

async fn pair() -> (KcpStream, KcpStream) {
    let a = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let b = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let (tx, rx) = mpsc::channel(PACKETS);
    let source = a.local_addr().unwrap();
    let conv = 42;
    a.connect(b.local_addr().unwrap()).await.unwrap();
    let (left, ready) =
        KcpStream::start(a, b.local_addr().unwrap(), conv, KcpConfig::default(), None).unwrap();
    let (right, other) =
        KcpStream::start(b.clone(), source, conv, KcpConfig::default(), Some(rx)).unwrap();
    tokio::spawn(async move {
        let mut bytes = [0; 65536];
        loop {
            tokio::select! {
                _=tx.closed()=>break,
                packet=b.recv_from(&mut bytes)=> {
                    let (n,_)=packet.unwrap();
                    if tx.send(bytes[..n].to_vec()).await.is_err() {break;}
                }
            }
        }
    });
    timeout(Duration::from_secs(3), async {
        ready.await.unwrap().unwrap();
        other.await.unwrap().unwrap();
    })
    .await
    .unwrap();
    (left, right)
}

#[tokio::test]
async fn flush_half_close_and_response_drain_preserve_both_directions() {
    let (mut client, mut server) = pair().await;
    let receiver = tokio::spawn(async move {
        let mut input = Vec::new();
        server.read_to_end(&mut input).await.unwrap();
        assert_eq!(input, vec![7; 100_000]);
        server.write_all(b"reply").await.unwrap();
        server.flush().await.unwrap();
        server.shutdown().await.unwrap();
    });
    client.write_all(&vec![7; 100_000]).await.unwrap();
    timeout(Duration::from_secs(5), client.shutdown())
        .await
        .unwrap()
        .unwrap();
    let mut reply = Vec::new();
    timeout(Duration::from_secs(5), client.read_to_end(&mut reply))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reply, b"reply");
    receiver.await.unwrap();
}

#[tokio::test]
async fn nonreading_peer_bounds_writes_and_drop_cancels_blocked_transport() {
    let (mut client, server) = pair().await;
    let result = timeout(
        Duration::from_millis(250),
        client.write_all(&vec![3; 4 * 1024 * 1024]),
    )
    .await;
    assert!(result.is_err());
    drop(server);
    assert!(!matches!(
        timeout(Duration::from_secs(1), client.flush()).await,
        Ok(Ok(()))
    ));
    drop(client);
}

#[tokio::test]
async fn handshake_and_invalid_datagram_errors_terminate_worker() {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (tx, rx) = mpsc::channel(8);
    let (mut stream, ready) = KcpStream::start(
        socket,
        peer.local_addr().unwrap(),
        5,
        KcpConfig::default(),
        Some(rx),
    )
    .unwrap();
    tx.send(vec![0; 3]).await.unwrap();
    assert!(ready.await.is_err());
    assert!(stream.read(&mut [0]).await.is_err());
    assert!(stream.write(b"x").await.is_err());
}

#[tokio::test]
async fn wrong_conversation_is_ignored_but_invalid_session_message_closes() {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let (tx, rx) = mpsc::channel(8);
    let (mut stream, ready) = KcpStream::start(
        socket,
        peer.local_addr().unwrap(),
        5,
        KcpConfig::default(),
        Some(rx),
    )
    .unwrap();
    let mut header = KcpPacketHeader::new(9, KcpCmd::Push, 0, 128, 0, 0, 0, HELLO.len() as u32);
    let mut packet = header.to_bytes().unwrap().to_vec();
    packet.extend_from_slice(HELLO);
    tx.send(packet).await.unwrap();
    header.conv = 5;
    let mut packet = header.to_bytes().unwrap().to_vec();
    packet.extend_from_slice(HELLO);
    tx.send(packet).await.unwrap();
    ready.await.unwrap().unwrap();
    header.sn = 1;
    header.len = 1;
    let mut packet = header.to_bytes().unwrap().to_vec();
    packet.push(99);
    tx.send(packet).await.unwrap();
    assert!(
        timeout(Duration::from_secs(2), stream.read(&mut [0]))
            .await
            .unwrap()
            .is_err()
    );
}

#[tokio::test]
async fn idle_timeout_and_cancelled_connect_release_socket() {
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let local = socket.local_addr().unwrap();
    let peer = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    socket.connect(peer.local_addr().unwrap()).await.unwrap();
    let (stream, ready) = KcpStream::start(
        socket,
        peer.local_addr().unwrap(),
        7,
        KcpConfig {
            idle_timeout_secs: 1,
        },
        None,
    )
    .unwrap();
    assert!(
        timeout(Duration::from_secs(2), ready)
            .await
            .unwrap()
            .is_err()
    );
    drop(stream);
    timeout(Duration::from_secs(1), async {
        loop {
            if UdpSocket::bind(local).await.is_ok() {
                break;
            }
            sleep(Duration::from_millis(2)).await;
        }
    })
    .await
    .unwrap();
}

#[test]
fn adapter_chunks_fit_upstream_message_and_receive_window_limits() {
    let mut kcp = Kcp::new(1, kcp_ovo::KcpConfig::default()).unwrap();
    assert!(kcp.send(&vec![0; super::CHUNK + 1]).is_ok());
    assert!(kcp.waitsnd() < kcp.rcv_wnd as usize);
    assert!(
        KcpConfig {
            idle_timeout_secs: 0
        }
        .validate()
        .is_err()
    );
}

#[tokio::test]
async fn malformed_framework_frame_is_delivered_unchanged_for_shared_parser_rejection() {
    let (mut client, mut server) = pair().await;
    client.write_all(&[0, 0]).await.unwrap();
    let mut bytes = [0; 2];
    server.read_exact(&mut bytes).await.unwrap();
    let mut parser = crate::infrastructure::protocol::big_buf_reader::BigBufReader::new(9177, 7719);
    parser.extend_from_slice(&bytes).unwrap();
    assert!(parser.next_frame().is_err());
}
