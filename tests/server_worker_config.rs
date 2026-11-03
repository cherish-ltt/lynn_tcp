#![cfg(feature = "server")]

use std::time::Duration;

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

#[tokio::test]
#[allow(
    deprecated,
    reason = "Both public setters must enforce the same startup invariant"
)]
async fn zero_workers_fail_before_listening() {
    for legacy in [false, true] {
        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = reservation.local_addr().unwrap();
        drop(reservation);
        let builder = LynnServerConfigBuilder::new().with_addr(addr).unwrap();
        let config = if legacy {
            builder.with_server_max_threadpool_size(&0)
        } else {
            builder.with_server_max_taskpool_size(&0)
        }
        .build();
        let mut server = tokio::spawn(LynnServer::new_with_config(config).await.start());
        let result = timeout(Duration::from_millis(500), &mut server).await;
        let listening = TcpStream::connect(addr).await.is_ok();
        server.abort();
        assert!(!listening, "zero-worker server must not accept connections");
        assert!(matches!(
            result.unwrap().unwrap(),
            Err(lynn_tcp::LynnError::Config(_))
        ));
    }
}

#[tokio::test]
async fn positive_worker_counts_serve_requests() {
    async fn echo(input: InputBufVO) -> HandlerResult {
        HandlerResult::new_with_send(1, "ok".into(), vec![input.get_input_addr().unwrap()]).unwrap()
    }
    for workers in [&1, &4] {
        let reservation = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = reservation.local_addr().unwrap();
        drop(reservation);
        let config = LynnServerConfigBuilder::new()
            .with_addr(addr)
            .unwrap()
            .with_server_max_taskpool_size(workers)
            .with_max_connections_per_ip(&0)
            .with_connection_rate_limit(&0)
            .build();
        let server = tokio::spawn(
            LynnServer::new_with_config(config)
                .await
                .add_router(1, echo)
                .start(),
        );
        let mut peer = timeout(Duration::from_secs(2), async {
            loop {
                if let Ok(peer) = TcpStream::connect(addr).await {
                    break peer;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let mut frame = 9177u16.to_le_bytes().to_vec();
        frame.extend_from_slice(&5u64.to_le_bytes());
        frame.extend_from_slice(&[1, 1, 0]);
        frame.extend_from_slice(&7719u16.to_le_bytes());
        peer.write_all(&frame).await.unwrap();
        let mut response = [0; 17];
        timeout(Duration::from_secs(2), peer.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response[13..15], b"ok");
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}
