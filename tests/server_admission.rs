#![cfg(feature = "server")]

use lynn_tcp::{
    lynn_server::{ClientsContext, LynnServer, LynnServerConfigBuilder},
    lynn_state::AppState,
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
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
    net::TcpStream,
    sync::Semaphore,
    task::JoinHandle,
    time::{sleep, timeout},
};

struct Work {
    started: AtomicUsize,
    gate: Semaphore,
}

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()])
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
}

async fn blocked(input: InputBufVO, work: AppState<Arc<Work>>) -> HandlerResult {
    work.started.fetch_add(1, Ordering::SeqCst);
    work.gate.acquire().await.unwrap().forget();
    echo(input).await
}

fn frame(method: u16) -> Vec<u8> {
    let mut data = 9177u16.to_le_bytes().to_vec();
    data.extend_from_slice(&5u64.to_le_bytes());
    data.push(1);
    data.extend_from_slice(&method.to_le_bytes());
    data.extend_from_slice(&7719u16.to_le_bytes());
    data
}

async fn receive(socket: &mut TcpStream) -> (u16, u16, Vec<u8>) {
    timeout(Duration::from_secs(2), async {
        let mut head = [0; 10];
        socket.read_exact(&mut head).await.unwrap();
        let mut body = vec![0; u64::from_le_bytes(head[2..].try_into().unwrap()) as usize];
        socket.read_exact(&mut body).await.unwrap();
        (
            u16::from_le_bytes(head[..2].try_into().unwrap()),
            u16::from_le_bytes(body[body.len() - 2..].try_into().unwrap()),
            body[3..body.len() - 2].to_vec(),
        )
    })
    .await
    .expect("response timeout")
}

async fn start(cap: &'static usize) -> (SocketAddr, Arc<Work>, JoinHandle<()>) {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let work = Arc::new(Work {
        started: AtomicUsize::new(0),
        gate: Semaphore::new(0),
    });
    let server = LynnServer::new_with_config(
        LynnServerConfigBuilder::new()
            .with_addr(addr)
            .unwrap()
            .with_server_max_connections(Some(cap))
            .with_server_max_taskpool_size(&1)
            .with_max_connections_per_ip(&0)
            .with_connection_rate_limit(&0)
            .with_server_check_heart_timeout_time(&300)
            .build(),
    )
    .await
    .with_state(work.clone())
    .add_router(1, echo)
    .add_router(2, online)
    .add_router(3, blocked);
    let task = tokio::spawn(server.start());
    sleep(Duration::from_millis(100)).await;
    (addr, work, task)
}

#[tokio::test]
async fn global_connection_limit_counts_pending_accepts() {
    let (addr, work, server) = start(&2).await;
    let mut blocker = TcpStream::connect(addr).await.unwrap();
    blocker.write_all(&frame(3)).await.unwrap();
    timeout(Duration::from_secs(2), async {
        while work.started.load(Ordering::SeqCst) == 0 {
            sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
    let mut pending = Vec::new();
    for _ in 0..5 {
        pending.push(TcpStream::connect(addr).await.unwrap());
    }
    sleep(Duration::from_millis(100)).await;
    work.gate.add_permits(1);
    receive(&mut blocker).await;
    sleep(Duration::from_millis(100)).await;
    blocker.write_all(&frame(2)).await.unwrap();
    let (_, _, count) = receive(&mut blocker).await;
    let count: usize = String::from_utf8(count).unwrap().parse().unwrap();
    eprintln!(
        "global max=2; online={count}; pending peers={}",
        pending.len()
    );
    server.abort();
    assert_eq!(count, 2);
}

#[tokio::test]
async fn global_slot_is_reused_after_eof_and_invalid_frame() {
    let (addr, _, server) = start(&1).await;
    for invalid in [false, true, false, true] {
        let mut peer = TcpStream::connect(addr).await.unwrap();
        peer.write_all(&frame(1)).await.unwrap();
        assert_eq!(receive(&mut peer).await.2, b"ok");
        if invalid {
            let mut bad = frame(1);
            *bad.last_mut().unwrap() = 0;
            peer.write_all(&bad).await.unwrap();
            assert_eq!(
                timeout(Duration::from_secs(2), peer.read(&mut [0]))
                    .await
                    .unwrap()
                    .unwrap(),
                0
            );
        }
        drop(peer);
        sleep(Duration::from_millis(100)).await;
    }
    server.abort();
}
