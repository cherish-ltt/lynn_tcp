#![cfg(feature = "server")]

use std::{
    future::Future,
    net::SocketAddr,
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use lynn_tcp::{
    lynn_middleware::{MiddlewareChain, MiddlewareRequest, Next, RequestContext},
    lynn_server::{ClientsContext, LynnServer, LynnServerConfigBuilder},
    lynn_state::AppState,
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::Barrier,
    time::{sleep, timeout},
};

#[derive(Default)]
struct Events(Mutex<Vec<String>>);

impl Events {
    fn record(&self, value: impl Into<String>) {
        self.0.lock().unwrap().push(value.into());
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap())
    }
}

type TestFuture = Pin<Box<dyn Future<Output = lynn_tcp::Result<HandlerResult>> + Send>>;

fn layer(name: &'static str) -> impl Fn(MiddlewareRequest, Next) -> TestFuture {
    move |request, next| {
        Box::pin(async move {
            let events = request.app_state::<Arc<Events>>().unwrap();
            events.record(format!("{name} in"));
            tokio::task::yield_now().await;
            let result = next.run(request).await;
            tokio::task::yield_now().await;
            events.record(format!("{name} out"));
            result
        })
    }
}

async fn configured() -> (SocketAddr, LynnServer<'static>) {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)
        .unwrap()
        .with_server_max_taskpool_size(&2)
        .with_connection_rate_limit(&0)
        .with_max_connections_per_ip(&0)
        .with_server_check_heart_timeout_time(&300)
        .build();
    (addr, LynnServer::new_with_config(config).await)
}

async fn connect(addr: SocketAddr) -> TcpStream {
    timeout(Duration::from_secs(3), async {
        loop {
            if let Ok(peer) = TcpStream::connect(addr).await {
                return peer;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap()
}

fn frame(method: u16, payload: &[u8]) -> Vec<u8> {
    let mut data = 9177u16.to_le_bytes().to_vec();
    data.extend_from_slice(&(payload.len() as u64 + 5).to_le_bytes());
    data.push(1);
    data.extend_from_slice(&method.to_le_bytes());
    data.extend_from_slice(payload);
    data.extend_from_slice(&7719u16.to_le_bytes());
    data
}

async fn receive(peer: &mut TcpStream) -> (u16, Vec<u8>) {
    timeout(Duration::from_secs(3), async {
        let mut head = [0; 10];
        peer.read_exact(&mut head).await.unwrap();
        assert_eq!(u16::from_le_bytes(head[..2].try_into().unwrap()), 9177);
        let mut body = vec![0; u64::from_le_bytes(head[2..].try_into().unwrap()) as usize];
        peer.read_exact(&mut body).await.unwrap();
        assert_eq!(&body[body.len() - 2..], &7719u16.to_le_bytes());
        (
            u16::from_le_bytes(body[1..3].try_into().unwrap()),
            body[3..body.len() - 2].to_vec(),
        )
    })
    .await
    .expect("response timeout")
}

async fn echo(context: RequestContext, events: AppState<Arc<Events>>) -> HandlerResult {
    events.record("handler");
    HandlerResult::new_with_send(context.method_id(), "ok".into(), vec![context.peer_addr()])
        .unwrap()
}

#[tokio::test]
async fn global_local_order_short_circuit_and_complete_duplicate_route_replacement() {
    let (addr, server) = configured().await;
    let events = Arc::new(Events::default());
    let server = server
        .with_state(events.clone())
        .with_middleware(layer("G1"))
        .add_router_with_middleware(
            1,
            echo,
            MiddlewareChain::new().with(layer("R1")).with(layer("R2")),
        )
        .add_router_with_middleware(2, echo, MiddlewareChain::new().with(layer("old")))
        .add_router(2, echo)
        .add_router_with_middleware(3, echo, MiddlewareChain::new().with(layer("old")))
        .add_router_with_middleware(
            3,
            echo,
            MiddlewareChain::new().with(layer("new")).with(layer("new")),
        )
        .add_router_with_middleware(
            4,
            echo,
            MiddlewareChain::new()
                .with(|request: MiddlewareRequest, _next: Next| async move {
                    request
                        .app_state::<Arc<Events>>()
                        .unwrap()
                        .record("short circuit");
                    HandlerResult::new_with_send(4, "ok".into(), vec![request.peer_addr()])
                })
                .with(layer("inner")),
        )
        .with_middleware(layer("G2"));
    let task = tokio::spawn(server.start());
    let mut peer = connect(addr).await;
    for (method, expected) in [
        (
            1,
            vec![
                "G1 in", "G2 in", "R1 in", "R2 in", "handler", "R2 out", "R1 out", "G2 out",
                "G1 out",
            ],
        ),
        (2, vec!["G1 in", "G2 in", "handler", "G2 out", "G1 out"]),
        (
            3,
            vec![
                "G1 in", "G2 in", "new in", "new in", "handler", "new out", "new out", "G2 out",
                "G1 out",
            ],
        ),
        (
            4,
            vec!["G1 in", "G2 in", "short circuit", "G2 out", "G1 out"],
        ),
    ] {
        peer.write_all(&frame(method, &[])).await.unwrap();
        assert_eq!(receive(&mut peer).await, (method, b"ok".to_vec()));
        assert_eq!(events.take(), expected);
    }
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

struct Instance(u8);
struct Identity(u8);

async fn identity(mut request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
    assert!(request.get::<Identity>().is_none());
    assert!(request.app_state::<u64>().is_none());
    let value = request.payload()[0];
    assert!(request.insert(Identity(value)).is_none());
    assert_eq!(request.insert(Identity(value + 1)).unwrap().0, value);
    tokio::task::yield_now().await;
    assert_eq!(request.get::<Identity>().unwrap().0, value + 1);
    next.run(request).await
}

async fn isolated(
    context: RequestContext,
    mut input: InputBufVO,
    clients: ClientsContext,
    instance: AppState<Instance>,
    barrier: AppState<Arc<Barrier>>,
) -> HandlerResult {
    let value = input.next_u8().unwrap();
    assert_eq!(input.get_remaining_data_len(), 0);
    assert_eq!(context.method_id(), 7);
    assert_eq!(Some(context.peer_addr()), input.get_input_addr());
    assert!(
        clients
            .get_all_clients_addrs()
            .await
            .contains(&context.peer_addr())
    );
    assert_eq!(context.get::<Identity>().unwrap().0, value + 1);
    assert!(context.get::<u64>().is_none());
    assert!(Arc::ptr_eq(
        &context.get::<Identity>().unwrap(),
        &context.clone().get::<Identity>().unwrap(),
    ));
    barrier.wait().await;
    HandlerResult::new_with_send(
        7,
        vec![instance.0, value, context.get::<Identity>().unwrap().0].into(),
        vec![context.peer_addr()],
    )
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn request_data_and_app_state_stay_isolated_across_concurrent_instances() {
    let barrier = Arc::new(Barrier::new(4));
    let mut servers = Vec::new();
    let mut clients = Vec::new();
    for instance in [10, 20] {
        let (addr, server) = configured().await;
        servers.push(tokio::spawn(
            server
                .with_state(Instance(instance))
                .with_state(barrier.clone())
                .add_router(7, isolated)
                .with_middleware(identity)
                .start(),
        ));
        for value in [1, 2] {
            let mut peer = connect(addr).await;
            clients.push(tokio::spawn(async move {
                peer.write_all(&frame(7, &[value])).await.unwrap();
                assert_eq!(
                    receive(&mut peer).await,
                    (7, vec![instance, value, value + 1])
                );
            }));
        }
    }
    for client in clients {
        client.await.unwrap();
    }
    for server in servers {
        server.abort();
        assert!(server.await.unwrap_err().is_cancelled());
    }
}

#[tokio::test]
async fn context_without_middleware_is_empty_and_preserves_old_payload_cursor() {
    let (addr, server) = configured().await;
    let task = tokio::spawn(
        server
            .add_router(
                9,
                |context: RequestContext, mut input: InputBufVO| async move {
                    assert!(context.get::<Identity>().is_none());
                    assert_eq!(context.method_id(), 9);
                    assert_eq!(Some(context.peer_addr()), input.get_input_addr());
                    assert_eq!(input.next_u8(), Some(42));
                    HandlerResult::new_with_send(9, "empty".into(), vec![context.peer_addr()])
                        .unwrap()
                },
            )
            .start(),
    );
    let mut peer = connect(addr).await;
    peer.write_all(&frame(9, &[42])).await.unwrap();
    assert_eq!(receive(&mut peer).await, (9, b"empty".to_vec()));
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn no_send_and_multi_target_results_pass_through_the_chain_only_once() {
    let (addr, server) = configured().await;
    let events = Arc::new(Events::default());
    let task = tokio::spawn(
        server
            .with_state(events.clone())
            .with_middleware(layer("outer"))
            .add_router(1, |events: AppState<Arc<Events>>| async move {
                events.record("no send");
                HandlerResult::new_without_send()
            })
            .add_router(
                2,
                |clients: ClientsContext, events: AppState<Arc<Events>>| async move {
                    events.record("multi target");
                    HandlerResult::new_with_send(
                        2,
                        "both".into(),
                        clients.get_all_clients_addrs().await,
                    )
                    .unwrap()
                },
            )
            .start(),
    );
    let mut first = connect(addr).await;
    let mut second = connect(addr).await;
    first.write_all(&frame(1, &[])).await.unwrap();
    timeout(Duration::from_secs(3), async {
        while events.0.lock().unwrap().len() != 3 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(events.take(), ["outer in", "no send", "outer out"]);
    second.write_all(&frame(2, &[])).await.unwrap();
    assert_eq!(receive(&mut first).await, (2, b"both".to_vec()));
    assert_eq!(receive(&mut second).await, (2, b"both".to_vec()));
    assert_eq!(events.take(), ["outer in", "multi target", "outer out"]);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[test]
fn response_view_borrows_validated_business_fields() {
    let addr = "127.0.0.1:1234".parse().unwrap();
    let result = HandlerResult::new_with_send(42, "body".into(), vec![addr, addr]).unwrap();
    let view = result.response_view().unwrap();
    assert_eq!(view.method_id(), 42);
    assert_eq!(view.payload(), b"body");
    assert_eq!(view.target_addrs(), &[addr, addr]);
    assert!(HandlerResult::new_without_send().response_view().is_none());
    assert!(
        HandlerResult::new_with_send(1, vec![0; 10 * 1024 * 1024 - 4].into(), vec![addr]).is_err()
    );
}

#[tokio::test]
async fn replaced_response_keeps_server_instance_protocol_marks() {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let task = tokio::spawn(
        LynnServer::new_with_config(
            LynnServerConfigBuilder::new()
                .with_addr(addr)
                .unwrap()
                .with_message_header_mark(&0x1234)
                .with_message_tail_mark(&0x5678)
                .with_server_max_taskpool_size(&1)
                .with_connection_rate_limit(&0)
                .with_max_connections_per_ip(&0)
                .build(),
        )
        .await
        .add_router(1, |context: RequestContext| async move {
            HandlerResult::new_with_send(1, "original".into(), vec![context.peer_addr()]).unwrap()
        })
        .with_middleware(|request, next: Next| async move {
            let result = next.run(request).await?;
            let view = result.response_view().unwrap();
            assert_eq!(view.payload(), b"original");
            HandlerResult::new_with_send(2, "replaced".into(), view.target_addrs().to_vec())
        })
        .start(),
    );
    let mut peer = connect(addr).await;
    let mut request = frame(1, &[]);
    request[..2].copy_from_slice(&0x1234u16.to_le_bytes());
    request[13..].copy_from_slice(&0x5678u16.to_le_bytes());
    peer.write_all(&request).await.unwrap();
    let mut response = [0; 23];
    timeout(Duration::from_secs(3), peer.read_exact(&mut response))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&response[..2], &0x1234u16.to_le_bytes());
    assert_eq!(&response[10..13], &[1, 2, 0]);
    assert_eq!(&response[13..21], b"replaced");
    assert_eq!(&response[21..], &0x5678u16.to_le_bytes());
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}
