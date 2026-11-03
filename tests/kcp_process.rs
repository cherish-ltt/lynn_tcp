#![cfg(all(feature = "client", feature = "server", feature = "kcp"))]

use lynn_tcp::{
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_kcp::KcpConfig,
    lynn_tcp_dependents::{HandlerResult, InputBufVOTrait},
};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::Duration,
};
use tokio::time::{sleep, timeout};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn independent_server_process_supports_tcp_and_kcp_without_handler_changes() {
    for (transport, mode) in [("tcp", "fast"), ("kcp", "fast"), ("kcp", "normal")] {
        let mut server = Server(
            Command::new(env!("CARGO_BIN_EXE_bench_echo_server"))
                .env("LYNN_BENCH_TRANSPORT", transport)
                .env("LYNN_BENCH_KCP_MODE", mode)
                .stdout(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let stdout = server.0.stdout.take().unwrap();
        let line = timeout(
            Duration::from_secs(10),
            tokio::task::spawn_blocking(move || {
                let mut line = String::new();
                BufReader::new(stdout).read_line(&mut line)?;
                Ok::<_, std::io::Error>(line)
            }),
        )
        .await
        .expect("server readiness deadline")
        .unwrap()
        .unwrap();
        let port: u16 = line
            .trim()
            .strip_prefix("READY ")
            .expect("server exited without READY; check child stderr")
            .parse()
            .unwrap();
        let addr = format!("127.0.0.1:{port}");
        let mut tasks = tokio::task::JoinSet::new();
        for id in 0..16u8 {
            let addr = addr.clone();
            tasks.spawn(async move {
                let mut peer = timeout(Duration::from_secs(10), async {
                    loop {
                        let config = LynnClientConfigBuilder::new()
                            .with_server_addr(&addr)
                            .unwrap()
                            .with_reconnect_max_attempts(&1);
                        let config = if transport == "kcp" {
                            config.with_kcp(if mode == "normal" {
                                KcpConfig::normal_mode()
                            } else {
                                KcpConfig::default()
                            })
                        } else {
                            config
                        };
                        let peer = LynnClient::new_with_config(config.build())
                            .await
                            .start()
                            .await;
                        if peer.is_connected() {
                            break peer;
                        }
                        sleep(Duration::from_millis(20)).await;
                    }
                })
                .await
                .unwrap();
                for sequence in 0..50u8 {
                    let payload = vec![id, sequence, 0, 255];
                    peer.send_data(
                        HandlerResult::new_with_send_to_server(1, payload.clone().into()).unwrap(),
                    )
                    .await
                    .unwrap();
                    let response = timeout(Duration::from_secs(5), peer.get_receive_data())
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(response.get_all_bytes().as_ref(), payload);
                }
            });
        }
        timeout(Duration::from_secs(30), async {
            while let Some(task) = tasks.join_next().await {
                task.unwrap();
            }
        })
        .await
        .unwrap();
        assert!(server.0.try_wait().unwrap().is_none());
        println!("{transport}/{mode}: independent process, 16 clients, 800 exact responses");
    }
}
