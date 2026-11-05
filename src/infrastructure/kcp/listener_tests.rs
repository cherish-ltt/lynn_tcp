use super::*;
use tokio::time::{sleep, timeout};

#[tokio::test]
async fn invalid_handshake_conv_isolation_and_per_ip_permits_follow_session_owner() {
    let listener = KcpListener::bind("127.0.0.1:0", KcpConfig::default())
        .await
        .unwrap();
    let addr = listener.socket.local_addr().unwrap();
    let queue = Arc::new(crossbeam_deque::Injector::new());
    let limiter = Arc::new(ConnectionLimiter::new(0, 1));
    let task = tokio::spawn(listener.run(Some(2), Some(limiter), queue.clone()));
    let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    socket.connect(addr).await.unwrap();
    socket.send(&[0; 27]).await.unwrap();
    let invalid = KcpPacketHeader::new(1, KcpCmd::Push, 0, 128, 0, 0, 0, 3);
    let mut packet = invalid.to_bytes().unwrap();
    packet.extend_from_slice(b"bad");
    socket.send(&packet).await.unwrap();
    for declared in [HELLO.len() - 1, HELLO.len() + 1] {
        let header = KcpPacketHeader::new(1, KcpCmd::Push, 0, 128, 0, 0, 0, declared as u32);
        let mut packet = header.to_bytes().unwrap();
        packet.extend_from_slice(HELLO);
        socket.send(&packet).await.unwrap();
    }
    for declared in [HELLO.len(), HELLO.len() + 1] {
        let header = KcpPacketHeader::new(1, KcpCmd::Push, 0, 128, 0, 0, 0, declared as u32);
        let mut packet = header.to_bytes().unwrap();
        packet.extend_from_slice(HELLO);
        packet.push(0);
        socket.send(&packet).await.unwrap();
    }
    sleep(Duration::from_millis(30)).await;
    assert!(queue.is_empty());

    let (first, ready) =
        KcpStream::start(socket.clone(), addr, 42, KcpConfig::default(), None).unwrap();
    timeout(Duration::from_secs(3), ready)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(queue.len(), 1);

    let different_conv =
        KcpPacketHeader::new(43, KcpCmd::Push, 0, 128, 0, 0, 0, HELLO.len() as u32);
    let mut packet = different_conv.to_bytes().unwrap();
    packet.extend_from_slice(HELLO);
    socket.send(&packet).await.unwrap();
    let other = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    other.connect(addr).await.unwrap();
    other.send(&packet).await.unwrap();
    sleep(Duration::from_millis(100)).await;
    assert_eq!(
        queue.len(),
        1,
        "active conv and per-IP permit must not be replaced"
    );

    drop(first);
    drop(queue.steal().success().unwrap());
    let (replacement, ready) =
        KcpStream::start(socket, addr, 43, KcpConfig::default(), None).unwrap();
    timeout(Duration::from_secs(3), ready)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        queue.len(),
        1,
        "closed same-address session must release its permit"
    );
    drop(replacement);
    drop(queue.steal().success().unwrap());
    task.abort();
    let _ = task.await;
}
