//! Integration tests: full LynnServer ↔ LynnClient round trips over real TCP sockets.
//!
//! Each test binds its own server on a unique 127.0.0.1 port so tests can run in
//! parallel within this binary. All servers here use the default message marks
//! (9177 / 7719); tests with custom marks live in a separate binary because the
//! server-side marks are process-global (`OnceLock`).

use std::{net::SocketAddr, time::Duration};

use lynn_tcp::{
    bytes::Bytes,
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_server::{ClientsContext, LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
    tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::{TcpListener, TcpStream},
        time::timeout,
    },
};

const HEADER_MARK: u16 = 9177;
const TAIL_MARK: u16 = 7719;
/// Static constants used to satisfy the `&'a` config builder lifetimes.
static PER_IP_ONE: usize = 1;
static MAX_CONN_ZERO: usize = 0;
static PERMIT_ZERO: usize = 0;
static RATE_ZERO: u64 = 0;

// ── helpers ─────────────────────────────────────────────────────────────

fn encode_frame(constructor_id: u8, method_id: u16, body: &[u8]) -> Vec<u8> {
    let msg_len = (1 + 2 + body.len() + 2) as u64;
    let mut frame = Vec::with_capacity(10 + msg_len as usize);
    frame.extend_from_slice(&HEADER_MARK.to_le_bytes());
    frame.extend_from_slice(&msg_len.to_le_bytes());
    frame.push(constructor_id);
    frame.extend_from_slice(&method_id.to_le_bytes());
    frame.extend_from_slice(body);
    frame.extend_from_slice(&TAIL_MARK.to_le_bytes());
    frame
}

/// Reads one full frame (header + body) from a raw socket.
async fn read_frame(stream: &mut TcpStream) -> (u8, u16, Vec<u8>) {
    let mut head = [0u8; 10];
    stream
        .read_exact(&mut head)
        .await
        .expect("read frame header");
    assert_eq!(
        u16::from_le_bytes([head[0], head[1]]),
        HEADER_MARK,
        "unexpected header mark"
    );
    let msg_len = u64::from_le_bytes(head[2..10].try_into().unwrap()) as usize;
    assert!(msg_len >= 4, "msg_len too small: {msg_len}");
    let mut rest = vec![0u8; msg_len];
    stream.read_exact(&mut rest).await.expect("read frame body");
    let constructor_id = rest[0];
    let method_id = u16::from_le_bytes([rest[1], rest[2]]);
    (constructor_id, method_id, rest[3..msg_len - 2].to_vec())
}

/// Polls until the server accepts TCP connections (or fails after ~6s).
async fn wait_server(addr: &str) {
    for _ in 0..120 {
        if TcpStream::connect(addr).await.is_ok() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never became reachable at {addr}");
}

async fn start_client(addr: &str) -> LynnClient<'static> {
    LynnClient::new_with_addr(addr.to_string())
        .await
        .start()
        .await
}

/// Receives one server response and returns its payload body as a String.
async fn recv_body(client: &mut LynnClient<'_>) -> String {
    let resp = timeout(Duration::from_secs(5), client.get_receive_data())
        .await
        .expect("timed out waiting for a server response")
        .expect("server closed the connection");
    String::from_utf8_lossy(&resp.get_all_bytes()).to_string()
}

/// Asserts that no server response arrives within `millis`.
async fn assert_no_response(client: &mut LynnClient<'_>, millis: u64) {
    let got = timeout(Duration::from_millis(millis), client.get_receive_data()).await;
    assert!(got.is_err(), "expected no response, got one");
}

async fn send_payload(client: &mut LynnClient<'_>, method_id: u16, payload: &str) {
    client
        .send_data(HandlerResult::new_with_send_to_server(
            method_id,
            Bytes::from(payload.to_owned()),
        ))
        .await
        .expect("client send failed");
}

async fn spawn_default_server(
    port: u16,
    register: impl FnOnce(LynnServer<'static>) -> LynnServer<'static> + Send + 'static,
) -> String {
    let addr = format!("127.0.0.1:{port}");
    let server_addr = addr.clone();
    tokio::spawn(async move {
        let server = LynnServer::new_with_addr(server_addr).await;
        register(server).start().await;
    });
    wait_server(&addr).await;
    addr
}

// ── handlers used across tests ──────────────────────────────────────────

async fn echo_two_params(input_buf_vo: InputBufVO, clients: ClientsContext) -> HandlerResult {
    let payload = String::from_utf8_lossy(&input_buf_vo.get_all_bytes()).to_string();
    let addrs = clients.get_all_clients_addrs().await;
    HandlerResult::new_with_send(1, format!("Echo: {payload}").into(), addrs)
}

async fn echo_single_param(input_buf_vo: InputBufVO) -> HandlerResult {
    let payload = String::from_utf8_lossy(&input_buf_vo.get_all_bytes()).to_string();
    let addr = input_buf_vo
        .get_input_addr()
        .map(|a: SocketAddr| a.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    HandlerResult::new_with_send(
        2,
        format!("Echo[{addr}]: {payload}").into(),
        vec![input_buf_vo.get_input_addr().unwrap()],
    )
}

async fn broadcast(clients: ClientsContext) -> HandlerResult {
    let addrs = clients.get_all_clients_addrs().await;
    HandlerResult::new_with_send(3, "hi all".into(), addrs)
}

async fn broadcast_reversed(_clients: ClientsContext, _input: InputBufVO) -> HandlerResult {
    HandlerResult::new_without_send()
}

async fn silent() -> HandlerResult {
    HandlerResult::new_without_send()
}

async fn echo_with_fake_addr(input_buf_vo: InputBufVO, clients: ClientsContext) -> HandlerResult {
    let payload = String::from_utf8_lossy(&input_buf_vo.get_all_bytes()).to_string();
    let mut addrs = clients.get_all_clients_addrs().await;
    // A well-formed address that is not in the clients map: exercises the
    // "delayed socket" warning branch of the send path.
    addrs.push("192.0.2.1:65535".parse().unwrap());
    HandlerResult::new_with_send(1, format!("Echo: {payload}").into(), addrs)
}

// ── tests ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn echo_roundtrip_with_two_param_handler() {
    let addr = spawn_default_server(9301, |s| {
        s.add_router(1, echo_two_params)
            .add_router(9, broadcast_reversed)
    })
    .await;
    let mut client = start_client(&addr).await;

    send_payload(&mut client, 1, "hello lynn").await;
    let body = recv_body(&mut client).await;
    assert_eq!(body, "Echo: hello lynn");

    // The client-side InputBufVO exposes the method id of the frame.
    send_payload(&mut client, 1, "again").await;
    let mut resp = timeout(Duration::from_secs(5), client.get_receive_data())
        .await
        .expect("timeout")
        .expect("connection closed");
    assert_eq!(resp.get_method_id(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&resp.get_all_bytes()),
        "Echo: again"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn single_param_handler_replies_to_sender() {
    let addr = spawn_default_server(9302, |s| s.add_router(2, echo_single_param)).await;
    let mut client = start_client(&addr).await;

    send_payload(&mut client, 2, "solo").await;
    let body = recv_body(&mut client).await;
    assert!(body.starts_with("Echo["), "unexpected body: {body}");
    assert!(body.ends_with(": solo"), "unexpected body: {body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clients_context_only_handler_broadcasts() {
    let addr = spawn_default_server(9303, |s| s.add_router(3, broadcast)).await;
    let mut client = start_client(&addr).await;

    send_payload(&mut client, 3, "ignored").await;
    let body = recv_body(&mut client).await;
    assert_eq!(body, "hi all");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn broadcast_reaches_every_connected_client() {
    let addr = spawn_default_server(9307, |s| s.add_router(3, broadcast)).await;
    let mut c1 = start_client(&addr).await;
    let mut c2 = start_client(&addr).await;

    send_payload(&mut c1, 3, "go").await;
    send_payload(&mut c2, 3, "go").await;
    assert_eq!(recv_body(&mut c1).await, "hi all");
    assert_eq!(recv_body(&mut c2).await, "hi all");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn handler_without_send_produces_no_response() {
    let addr = spawn_default_server(9304, |s| s.add_router(1, silent)).await;
    let mut client = start_client(&addr).await;

    send_payload(&mut client, 1, "silent").await;
    assert_no_response(&mut client, 700).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_method_id_gets_no_response() {
    let addr = spawn_default_server(9305, |s| s.add_router(1, echo_two_params)).await;
    let mut client = start_client(&addr).await;

    send_payload(&mut client, 99, "nobody home").await;
    assert_no_response(&mut client, 700).await;

    // The connection stays usable for registered routes.
    send_payload(&mut client, 1, "still there?").await;
    assert_eq!(recv_body(&mut client).await, "Echo: still there?");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unknown_constructor_id_is_skipped() {
    let addr = spawn_default_server(9306, |s| s.add_router(1, echo_two_params)).await;
    let mut raw = TcpStream::connect(&addr).await.expect("connect");
    // constructor_id 5 is not defined by the protocol: must be skipped without
    // tearing down the connection.
    raw.write_all(&encode_frame(5, 1, b"mystery"))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    raw.write_all(&encode_frame(1, 1, b"real")).await.unwrap();
    let (_, method, body) = read_frame(&mut raw).await;
    assert_eq!(method, 1);
    assert_eq!(String::from_utf8_lossy(&body), "Echo: real");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn response_to_unknown_addr_is_reported_not_fatal() {
    let addr = spawn_default_server(9308, |s| s.add_router(1, echo_with_fake_addr)).await;
    let mut client = start_client(&addr).await;

    send_payload(&mut client, 1, "plus one").await;
    assert_eq!(recv_body(&mut client).await, "Echo: plus one");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_process_permit_drops_requests() {
    let addr = "127.0.0.1:9309".to_string();
    let server_addr = addr.clone();
    tokio::spawn(async move {
        let config = LynnServerConfigBuilder::new()
            .with_addr(server_addr)
            .expect("addr")
            .with_server_single_processs_permit(&PERMIT_ZERO)
            .build();
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo_two_params)
            .start()
            .await;
    });
    wait_server(&addr).await;
    let mut client = start_client(&addr).await;

    send_payload(&mut client, 1, "no permit").await;
    assert_no_response(&mut client, 700).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn per_ip_connection_limit_rejects_extra_connections() {
    let addr = "127.0.0.1:9310".to_string();
    let server_addr = addr.clone();
    tokio::spawn(async move {
        let config = LynnServerConfigBuilder::new()
            .with_addr(server_addr)
            .expect("addr")
            .with_max_connections_per_ip(&PER_IP_ONE)
            .with_connection_rate_limit(&RATE_ZERO)
            .build();
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo_two_params)
            .start()
            .await;
    });
    // No probe here: wait_server's probe socket would consume the 1-per-IP budget.
    tokio::time::sleep(Duration::from_millis(400)).await;

    let mut first = TcpStream::connect(&addr).await.expect("first connect");
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut second = TcpStream::connect(&addr).await.expect("second connect");

    // The first connection stays open, the second is closed by the limiter.
    let mut probe = [0u8; 1];
    let still_open = timeout(Duration::from_millis(300), first.read(&mut probe)).await;
    assert!(still_open.is_err(), "first socket unexpectedly closed");
    let closed = timeout(Duration::from_secs(3), second.read(&mut probe))
        .await
        .expect("waiting for EOF timed out");
    assert_eq!(closed.unwrap_or(0), 0, "second socket should hit EOF");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn zero_max_connections_rejects_everyone() {
    let addr = "127.0.0.1:9311".to_string();
    let server_addr = addr.clone();
    tokio::spawn(async move {
        let config = LynnServerConfigBuilder::new()
            .with_addr(server_addr)
            .expect("addr")
            .with_server_max_connections(Some(&MAX_CONN_ZERO))
            .with_connection_rate_limit(&RATE_ZERO)
            .build();
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo_two_params)
            .start()
            .await;
    });
    tokio::time::sleep(Duration::from_millis(400)).await;

    let mut raw = TcpStream::connect(&addr).await.expect("connect");
    let mut probe = [0u8; 1];
    let closed = timeout(Duration::from_secs(3), raw.read(&mut probe))
        .await
        .expect("waiting for EOF timed out");
    assert_eq!(closed.unwrap_or(0), 0, "socket should be rejected (EOF)");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn client_heartbeat_keeps_connection_alive() {
    let addr = spawn_default_server(9312, |s| s.add_router(1, echo_two_params)).await;
    let heart_interval: u64 = 1;
    let mut client = LynnClient::new_with_config(
        LynnClientConfigBuilder::new()
            .with_server_addr(&addr)
            .expect("addr")
            // Heart frames (constructor_id 2) flow every second and must keep the
            // connection usable without producing user-visible responses.
            .with_server_check_heart_interval(&heart_interval)
            .build(),
    )
    .await
    .start()
    .await;

    tokio::time::sleep(Duration::from_millis(2_600)).await;

    send_payload(&mut client, 1, "after hearts").await;
    assert_eq!(recv_body(&mut client).await, "Echo: after hearts");
}

#[tokio::test]
async fn client_without_start_reports_errors() {
    let mut client = LynnClient::new_with_addr("127.0.0.1:9313").await;

    let err = client
        .send_data(HandlerResult::new_with_send_to_server(1, Bytes::new()))
        .await;
    assert!(err.is_err(), "send_data must fail before start()");

    let got = timeout(Duration::from_millis(100), client.get_receive_data()).await;
    match got {
        Ok(None) => {}, // not connected → immediate None
        Ok(Some(_)) => panic!("unexpected data received before start()"),
        Err(_) => panic!("get_receive_data should return immediately when not started"),
    }
}

#[tokio::test]
async fn client_connect_refused_leaves_client_unusable() {
    // Reserve a free port, then release it so nothing listens there.
    let free_port = {
        let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        l.local_addr().unwrap().port()
    };
    let addr = format!("127.0.0.1:{free_port}");

    let mut client = LynnClient::new_with_addr(addr.clone()).await.start().await;
    let err = client
        .send_data(HandlerResult::new_with_send_to_server(1, Bytes::new()))
        .await;
    assert!(
        err.is_err(),
        "send must fail when the server is unreachable"
    );
}

#[tokio::test]
#[should_panic(expected = "Invalid server address")]
async fn client_with_invalid_addr_panics() {
    let _ = LynnClient::new_with_addr("not a valid address".to_string()).await;
}
