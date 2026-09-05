//! Integration tests: client automatic reconnection (default 3 attempts).
//!
//! Uses a raw `TcpListener` so the tests control exactly when the "server"
//! drops or stops accepting, independent of the lynn_tcp server internals.

use std::time::{Duration, Instant};

use lynn_tcp::{
    bytes::Bytes,
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVOTrait},
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        time::timeout,
    },
};

/// Static constants used to satisfy the `&'a` config builder lifetimes.
static RECONNECT_ATTEMPTS: usize = 3;
static RECONNECT_INTERVAL: u64 = 0;
static CONNECT_TIMEOUT: u64 = 1;

async fn wait_until(desc: &str, secs: u64, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < deadline {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("condition not met within {secs}s: {desc}");
}

fn client_config(addr: &str) -> lynn_tcp::lynn_client::LynnClientConfig<'static> {
    LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .expect("addr")
        .with_reconnect_max_attempts(&RECONNECT_ATTEMPTS)
        .with_reconnect_interval_secs(&RECONNECT_INTERVAL)
        .with_connect_timeout_secs(&CONNECT_TIMEOUT)
        .build()
}

/// Reads one full lynn protocol frame from a raw socket and returns the bytes.
async fn read_one_frame(conn: &mut TcpStream) -> Vec<u8> {
    let mut head = [0u8; 10];
    conn.read_exact(&mut head).await.expect("read frame head");
    let msg_len = u64::from_le_bytes(head[2..10].try_into().unwrap()) as usize;
    let mut rest = vec![0u8; msg_len];
    conn.read_exact(&mut rest).await.expect("read frame body");
    let mut frame = head.to_vec();
    frame.extend_from_slice(&rest);
    frame
}

/// Reads user frames from a raw socket, discarding heartbeat frames the way a
/// real server would (constructor_id 2), until a user frame (constructor_id 1)
/// arrives. The client's first heartbeat tick fires immediately on start, so a
/// queued heartbeat may race a reconnect and surface on the new connection.
async fn read_user_frame(conn: &mut TcpStream) -> Vec<u8> {
    for _ in 0..5 {
        let frame = read_one_frame(conn).await;
        let constructor_id = frame[10];
        if constructor_id == 1 {
            return frame;
        }
    }
    panic!("no user frame arrived after 5 frames");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_reconnects_and_resumes_data_exchange() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().unwrap().to_string();

    let mut client = LynnClient::new_with_config(client_config(&addr))
        .await
        .start()
        .await;
    assert!(client.is_connected(), "initial connect must succeed");
    let (mut conn1, _) = timeout(Duration::from_secs(3), listener.accept())
        .await
        .expect("initial connect must be accepted")
        .expect("accept");

    // Simulate a server-side disconnect.
    conn1.shutdown().await.expect("shutdown conn1");
    drop(conn1);

    // The client automatically reconnects (attempt 1 of 3).
    let (mut conn2, _) = timeout(Duration::from_secs(5), listener.accept())
        .await
        .expect("no reconnect attempt within 5s")
        .expect("reconnect attempt failed");
    wait_until("reconnected", 5, || client.is_connected()).await;

    // The reconnected session must carry data end to end: raw echo server
    // (heartbeats, if any, are skipped like a real server would).
    client
        .send_data(HandlerResult::new_with_send_to_server(
            7,
            Bytes::from("after reconnect"),
        ))
        .await
        .expect("send must work after reconnect");
    let frame = read_user_frame(&mut conn2).await;
    conn2.write_all(&frame).await.expect("raw echo write");

    let response = timeout(Duration::from_secs(5), client.get_receive_data())
        .await
        .expect("timeout waiting for echo after reconnect")
        .expect("read half closed");
    assert_eq!(
        String::from_utf8_lossy(&response.get_all_bytes()),
        "after reconnect",
        "data must flow over the reconnected session"
    );
    assert!(client.is_connected());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_gives_up_after_max_attempts() {
    // Bind, accept one connection, then kill the "server" completely.
    let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = probe.local_addr().unwrap().to_string();
    let mut client = LynnClient::new_with_config(client_config(&addr))
        .await
        .start()
        .await;
    let (mut conn1, _) = timeout(Duration::from_secs(3), probe.accept())
        .await
        .expect("initial connect must be accepted")
        .expect("accept");
    assert!(client.is_connected());

    conn1.shutdown().await.expect("shutdown");
    drop(conn1);
    drop(probe);

    // Disconnect is detected...
    wait_until("disconnected after drop", 5, || !client.is_connected()).await;

    // ...3 reconnect attempts are refused (interval 0) and the supervisor
    // gives up. Give it a moment to burn through the attempts.
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(
        !client.is_connected(),
        "must stay disconnected after giving up"
    );
    let err = client
        .send_data(HandlerResult::new_with_send_to_server(1, Bytes::new()))
        .await;
    assert!(
        err.is_err(),
        "send must fail after reconnect attempts are exhausted"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn initial_connect_failure_leaves_client_disconnected() {
    // Reserve a port and release it so nothing listens there.
    let free_port = {
        let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        l.local_addr().unwrap().port()
    };
    let addr = format!("127.0.0.1:{free_port}");

    let mut client = LynnClient::new_with_config(client_config(&addr))
        .await
        .start()
        .await;
    assert!(!client.is_connected(), "initial connect must fail");
    let err = client
        .send_data(HandlerResult::new_with_send_to_server(1, Bytes::new()))
        .await;
    assert!(err.is_err(), "send must fail on a never-connected client");

    // get_receive_data must return promptly (None), not hang.
    let got = timeout(Duration::from_millis(300), client.get_receive_data()).await;
    assert!(
        got.is_ok(),
        "get_receive_data must not hang when disconnected"
    );
    assert!(got.unwrap().is_none());
}
