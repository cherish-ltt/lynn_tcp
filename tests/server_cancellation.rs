#![cfg(feature = "server")]

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder},
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
    calls: AtomicUsize,
    gate: Semaphore,
}

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()])
}

async fn blocked(input: InputBufVO, work: AppState<Arc<Work>>) -> HandlerResult {
    work.calls.fetch_add(1, Ordering::SeqCst);
    let _permit = work.gate.acquire().await.unwrap();
    echo(input).await
}

fn frame(method: u16) -> Vec<u8> {
    let mut frame = 9177u16.to_le_bytes().to_vec();
    frame.extend_from_slice(&5u64.to_le_bytes());
    frame.push(1);
    frame.extend_from_slice(&method.to_le_bytes());
    frame.extend_from_slice(&7719u16.to_le_bytes());
    frame
}

async fn start(work: Arc<Work>) -> (SocketAddr, JoinHandle<lynn_tcp::Result<()>>) {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&1)
        .with_server_check_heart_timeout_time(&300)
        .build();
    let server = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .with_state(work)
            .add_router(1, echo)
            .add_router(2, blocked)
            .start(),
    );
    (addr, server)
}

async fn connect(addr: SocketAddr) -> TcpStream {
    timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(peer) = TcpStream::connect(addr).await {
                return peer;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn cancellation_closes_existing_connections_and_releases_state() {
    for blocked_handler in [false, true] {
        let work = Arc::new(Work {
            calls: AtomicUsize::new(0),
            gate: Semaphore::new(0),
        });
        let weak = Arc::downgrade(&work);
        let (addr, server) = start(work.clone()).await;
        let mut peer = connect(addr).await;
        peer.write_all(&frame(1)).await.unwrap();
        timeout(Duration::from_secs(3), peer.read_exact(&mut [0; 17]))
            .await
            .unwrap()
            .unwrap();
        if blocked_handler {
            peer.write_all(&frame(2)).await.unwrap();
            timeout(Duration::from_secs(3), async {
                while work.calls.load(Ordering::SeqCst) == 0 {
                    sleep(Duration::from_millis(1)).await;
                }
            })
            .await
            .unwrap();
        }
        drop(work);
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
        assert!(TcpStream::connect(addr).await.is_err());
        assert_eq!(
            timeout(Duration::from_secs(3), peer.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
        timeout(Duration::from_secs(3), async {
            while weak.upgrade().is_some() {
                sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("server tasks retain AppState after cancellation");
    }
}

#[tokio::test]
async fn dropping_unstarted_server_releases_state() {
    let state = Arc::new(());
    let weak = Arc::downgrade(&state);
    drop(LynnServer::new().await.with_state(state));
    timeout(Duration::from_secs(3), async {
        while weak.upgrade().is_some() {
            sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .unwrap();
}
