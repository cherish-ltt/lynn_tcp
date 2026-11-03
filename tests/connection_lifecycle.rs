#![cfg(feature = "server")]

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use lynn_tcp::{
    lynn_server::{ClientsContext, LynnServer, LynnServerConfigBuilder},
    lynn_state::AppState,
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Semaphore,
    time::{sleep, timeout},
};

fn frame(method: u16) -> Vec<u8> {
    let mut frame = Vec::from(9177u16.to_le_bytes());
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.push(1);
    frame.extend_from_slice(&method.to_le_bytes());
    frame.extend_from_slice(&7719u16.to_le_bytes());
    frame
}

async fn receive(socket: &mut TcpStream) -> Vec<u8> {
    timeout(Duration::from_secs(3), async {
        let mut head = [0; 10];
        socket.read_exact(&mut head).await.unwrap();
        let mut body = vec![0; u64::from_le_bytes(head[2..].try_into().unwrap()) as usize];
        socket.read_exact(&mut body).await.unwrap();
        body[3..body.len() - 2].to_vec()
    })
    .await
    .expect("response timeout")
}

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()])
        .expect("payload fits the protocol limit")
}

async fn online(input: InputBufVO, clients: ClientsContext) -> HandlerResult {
    HandlerResult::new_with_send(
        2,
        clients
            .get_all_clients_addrs()
            .await
            .len()
            .to_string()
            .into(),
        vec![input.get_input_addr().unwrap()],
    )
    .expect("payload fits the protocol limit")
}

struct Work {
    active: AtomicUsize,
    peak: AtomicUsize,
    started: AtomicUsize,
    gate: Semaphore,
}

async fn blocked(input: InputBufVO, work: AppState<Arc<Work>>) -> HandlerResult {
    let active = work.active.fetch_add(1, Ordering::SeqCst) + 1;
    work.peak.fetch_max(active, Ordering::SeqCst);
    work.started.fetch_add(1, Ordering::SeqCst);
    work.gate.acquire().await.unwrap().forget();
    work.active.fetch_sub(1, Ordering::SeqCst);
    echo(input).await
}

async fn panics() -> HandlerResult {
    panic!("intentional handler panic")
}

async fn start(
    per_ip: &'static usize,
    permits: &'static usize,
    heart: &'static u64,
) -> (SocketAddr, Arc<Work>) {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let work = Arc::new(Work {
        active: AtomicUsize::new(0),
        peak: AtomicUsize::new(0),
        started: AtomicUsize::new(0),
        gate: Semaphore::new(0),
    });
    let server = LynnServer::new_with_config(
        LynnServerConfigBuilder::new()
            .with_addr(addr)
            .unwrap()
            .with_server_max_taskpool_size(&4)
            .with_max_connections_per_ip(per_ip)
            .with_connection_rate_limit(&0)
            .with_server_single_process_permit(permits)
            .with_server_check_heart_interval(&1)
            .with_server_check_heart_timeout_time(heart)
            .build(),
    )
    .await
    .with_state(work.clone())
    .add_router(1, echo)
    .add_router(2, online)
    .add_router(3, blocked)
    .add_router(4, panics);
    tokio::spawn(server.start());
    sleep(Duration::from_millis(100)).await;
    (addr, work)
}

#[tokio::test]
async fn per_ip_slot_is_reusable_after_eof() {
    let (addr, _) = start(&1, &4, &60).await;
    for _ in 0..4 {
        let mut socket = TcpStream::connect(addr).await.unwrap();
        socket.write_all(&frame(1)).await.unwrap();
        assert_eq!(receive(&mut socket).await, b"ok");
        drop(socket);
        sleep(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn eof_leaves_the_online_table_before_heartbeat() {
    let (addr, _) = start(&0, &4, &60).await;
    let mut first = TcpStream::connect(addr).await.unwrap();
    first.write_all(&frame(1)).await.unwrap();
    receive(&mut first).await;
    let mut observer = TcpStream::connect(addr).await.unwrap();
    observer.write_all(&frame(2)).await.unwrap();
    assert_eq!(receive(&mut observer).await, b"2");
    drop(first);
    sleep(Duration::from_millis(150)).await;
    observer.write_all(&frame(2)).await.unwrap();
    assert_eq!(receive(&mut observer).await, b"1");
}

#[tokio::test]
async fn processing_permit_covers_queued_and_running_work() {
    let (addr, work) = start(&0, &1, &60).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(&frame(3).repeat(8)).await.unwrap();
    sleep(Duration::from_millis(200)).await;
    assert_eq!(work.started.load(Ordering::SeqCst), 1);
    assert_eq!(work.peak.load(Ordering::SeqCst), 1);
    work.gate.add_permits(1);
    assert_eq!(receive(&mut socket).await, b"ok");
    socket.write_all(&frame(1)).await.unwrap();
    assert_eq!(receive(&mut socket).await, b"ok");
}

#[tokio::test]
async fn write_half_close_drains_already_received_response() {
    let (addr, work) = start(&1, &1, &60).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(&frame(3)).await.unwrap();
    socket.shutdown().await.unwrap();
    sleep(Duration::from_millis(150)).await;
    assert_eq!(work.started.load(Ordering::SeqCst), 1);
    work.gate.add_permits(1);
    assert_eq!(receive(&mut socket).await, b"ok");
    let mut byte = [0];
    assert_eq!(
        timeout(Duration::from_secs(1), socket.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn panic_returns_processing_permit() {
    let (addr, _) = start(&0, &1, &60).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(&frame(4)).await.unwrap();
    sleep(Duration::from_millis(100)).await;
    socket.write_all(&frame(1)).await.unwrap();
    assert_eq!(receive(&mut socket).await, b"ok");
}

#[tokio::test]
async fn processing_limits_are_independent_per_connection() {
    let (addr, work) = start(&0, &1, &60).await;
    let mut first = TcpStream::connect(addr).await.unwrap();
    let mut second = TcpStream::connect(addr).await.unwrap();
    first.write_all(&frame(3).repeat(4)).await.unwrap();
    second.write_all(&frame(3).repeat(4)).await.unwrap();
    sleep(Duration::from_millis(150)).await;
    assert_eq!(work.started.load(Ordering::SeqCst), 2);
    work.gate.add_permits(2);
    assert_eq!(receive(&mut first).await, b"ok");
    assert_eq!(receive(&mut second).await, b"ok");
}

#[tokio::test]
async fn heartbeat_closes_idle_read_half_and_reclaims_slot() {
    let (addr, _) = start(&1, &1, &1).await;
    let mut old = TcpStream::connect(addr).await.unwrap();
    old.write_all(&frame(1)).await.unwrap();
    assert_eq!(receive(&mut old).await, b"ok");
    let mut byte = [0];
    assert_eq!(
        timeout(Duration::from_secs(3), old.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    let mut next = TcpStream::connect(addr).await.unwrap();
    next.write_all(&frame(1)).await.unwrap();
    assert_eq!(receive(&mut next).await, b"ok");
}

#[tokio::test]
async fn zero_processing_permits_still_reject_requests_and_close_on_eof() {
    let (addr, _) = start(&1, &0, &60).await;
    let mut socket = TcpStream::connect(addr).await.unwrap();
    socket.write_all(&frame(1)).await.unwrap();
    socket.shutdown().await.unwrap();
    let mut byte = [0];
    assert_eq!(
        timeout(Duration::from_secs(1), socket.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}
