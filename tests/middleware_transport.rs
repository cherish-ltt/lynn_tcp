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
    lynn_middleware::{MiddlewareChain, MiddlewareRequest, Next, RequestContext},
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_state::AppState,
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
    sync::Semaphore,
    task::JoinHandle,
    time::{sleep, timeout},
};

struct Observations {
    middleware: AtomicUsize,
    handlers: AtomicUsize,
}

struct Identity(u8);

async fn authorize(mut request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
    let observations = request.app_state::<Arc<Observations>>().unwrap();
    observations.middleware.fetch_add(1, Ordering::SeqCst);
    assert_eq!(request.method_id(), 7);
    let addr = request.peer_addr();
    assert!(addr.ip().is_loopback());
    if request.payload() == b"denied" {
        return HandlerResult::new_with_send(7, "denied".into(), vec![addr]);
    }
    assert_eq!(request.payload(), b"accepted");
    request.insert(Identity(42));
    let result = next.run(request).await?;
    let view = result.response_view().unwrap();
    assert_eq!(view.method_id(), 7);
    assert_eq!(view.payload(), b"accepted");
    assert_eq!(view.target_addrs(), &[addr]);
    HandlerResult::new_with_send(7, "wrapped".into(), vec![addr])
}

async fn local(request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
    assert_eq!(request.get::<Identity>().unwrap().0, 42);
    next.run(request).await
}

async fn echo(
    input: InputBufVO,
    context: RequestContext,
    observations: AppState<Arc<Observations>>,
) -> HandlerResult {
    observations.handlers.fetch_add(1, Ordering::SeqCst);
    assert_eq!(context.get::<Identity>().unwrap().0, 42);
    assert_eq!(context.method_id(), 7);
    assert_eq!(Some(context.peer_addr()), input.get_input_addr());
    assert_eq!(input.get_all_bytes().as_ref(), b"accepted");
    HandlerResult::new_with_send(7, "accepted".into(), vec![context.peer_addr()]).unwrap()
}

async fn server(
    config: LynnServerConfigBuilder<'static>,
) -> (Arc<Observations>, JoinHandle<lynn_tcp::Result<()>>) {
    let observations = Arc::new(Observations {
        middleware: AtomicUsize::new(0),
        handlers: AtomicUsize::new(0),
    });
    let task = tokio::spawn(
        LynnServer::new_with_config(
            config
                .with_server_max_taskpool_size(&2)
                .with_connection_rate_limit(&0)
                .with_max_connections_per_ip(&0)
                .with_server_check_heart_timeout_time(&300)
                .build(),
        )
        .await
        .with_state(observations.clone())
        .add_router_with_middleware(7, echo, MiddlewareChain::new().with(local))
        .with_middleware(authorize)
        .start(),
    );
    (observations, task)
}

fn tcp_addr() -> SocketAddr {
    let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    addr
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

fn frame(constructor: u8, method: u16, payload: &[u8]) -> Vec<u8> {
    let mut data = 9177u16.to_le_bytes().to_vec();
    data.extend_from_slice(&(payload.len() as u64 + 5).to_le_bytes());
    data.push(constructor);
    data.extend_from_slice(&method.to_le_bytes());
    data.extend_from_slice(payload);
    data.extend_from_slice(&7719u16.to_le_bytes());
    data
}

async fn receive(peer: &mut (impl AsyncRead + Unpin)) -> Vec<u8> {
    timeout(Duration::from_secs(3), async {
        let mut head = [0; 10];
        peer.read_exact(&mut head).await.unwrap();
        let mut body = vec![0; u64::from_le_bytes(head[2..].try_into().unwrap()) as usize];
        peer.read_exact(&mut body).await.unwrap();
        body[3..body.len() - 2].to_vec()
    })
    .await
    .unwrap()
}

async fn exchange(peer: &mut (impl AsyncRead + AsyncWrite + Unpin)) {
    peer.write_all(&frame(1, 7, b"accepted")).await.unwrap();
    peer.flush().await.unwrap();
    assert_eq!(receive(peer).await, b"wrapped");
    peer.write_all(&frame(1, 7, b"denied")).await.unwrap();
    peer.flush().await.unwrap();
    assert_eq!(receive(peer).await, b"denied");
}

fn assert_observations(observations: &Observations, calls: usize) {
    assert_eq!(observations.middleware.load(Ordering::SeqCst), calls);
    assert_eq!(observations.handlers.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn tcp_context_short_circuit_and_response_replacement_cross_real_connection() {
    let addr = tcp_addr();
    let (observations, task) =
        server(LynnServerConfigBuilder::new().with_addr(addr).unwrap()).await;
    let mut peer = connect(addr).await;
    exchange(&mut peer).await;
    assert_observations(&observations, 2);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn payload_boundaries_preserve_cursor_and_oversized_replacement_keeps_connection_healthy() {
    const MAX_PAYLOAD: usize = 10 * 1024 * 1024 - 5;
    let inputs = Arc::new([
        Vec::new(),
        vec![b'a'; MAX_PAYLOAD],
        b"replace".to_vec(),
        b"healthy".to_vec(),
    ]);
    let calls = Arc::new(AtomicUsize::new(0));
    let errors = Arc::new(Semaphore::new(0));
    let addr = tcp_addr();
    let middleware_inputs = inputs.clone();
    let middleware_errors = errors.clone();
    let mut server = LynnServer::new_with_config(
        LynnServerConfigBuilder::new()
            .with_addr(addr)
            .unwrap()
            .with_server_max_taskpool_size(&2)
            .with_server_single_process_permit(&1)
            .with_connection_rate_limit(&0)
            .with_max_connections_per_ip(&0)
            // Test frame boundaries without constraining Linux TCP receive-window throughput.
            .with_recv_buffer_size(&0)
            .build(),
    )
    .await
    .with_middleware(move |request: MiddlewareRequest, next: Next| {
        let inputs = middleware_inputs.clone();
        let errors = middleware_errors.clone();
        async move {
            let method = request.method_id();
            let addr = request.peer_addr();
            let expected = &inputs[usize::from(method - 1)];
            assert_eq!(request.payload(), expected.as_slice());
            tokio::task::yield_now().await;
            assert_eq!(request.payload(), expected.as_slice());
            let result = next.run(request).await?;
            if method == 3 {
                let result = HandlerResult::new_with_send(
                    method,
                    vec![0; MAX_PAYLOAD + 1].into(),
                    vec![addr],
                );
                assert!(matches!(result, Err(lynn_tcp::LynnError::Protocol(_))));
                errors.add_permits(1);
                return result;
            }
            Ok(result)
        }
    });
    for method in 1..=4 {
        let inputs = inputs.clone();
        let calls = calls.clone();
        server = server.add_router(
            method,
            move |mut input: InputBufVO, context: RequestContext| {
                let inputs = inputs.clone();
                let calls = calls.clone();
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let expected = &inputs[usize::from(context.method_id() - 1)];
                    assert_eq!(input.get_all_bytes().as_ref(), expected.as_slice());
                    assert_eq!(input.get_remaining_data_len(), expected.len());
                    assert_eq!(input.next_u8(), expected.first().copied());
                    assert_eq!(
                        input.get_remaining_data_len(),
                        expected.len().saturating_sub(1)
                    );
                    HandlerResult::new_with_send(
                        context.method_id(),
                        format!("handled-{}", context.method_id()).into(),
                        vec![context.peer_addr()],
                    )
                    .unwrap()
                }
            },
        );
    }
    let task = tokio::spawn(server.start());
    let mut peer = connect(addr).await;
    for method in 1..=4 {
        timeout(
            Duration::from_secs(5),
            peer.write_all(&frame(1, method, &inputs[usize::from(method - 1)])),
        )
        .await
        .unwrap()
        .unwrap();
        if method == 3 {
            timeout(Duration::from_secs(3), errors.acquire())
                .await
                .unwrap()
                .unwrap()
                .forget();
        } else {
            assert_eq!(
                receive(&mut peer).await,
                format!("handled-{method}").as_bytes()
            );
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(errors.available_permits(), 0);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[tokio::test]
async fn heartbeat_unknown_constructor_unknown_route_and_invalid_frame_bypass_middleware() {
    let addr = tcp_addr();
    let (observations, task) =
        server(LynnServerConfigBuilder::new().with_addr(addr).unwrap()).await;
    let mut peer = connect(addr).await;
    peer.write_all(
        &[
            frame(2, 7, b"accepted"),
            frame(0, 7, b"accepted"),
            frame(1, 999, b"accepted"),
            frame(1, 7, b"accepted"),
        ]
        .concat(),
    )
    .await
    .unwrap();
    assert_eq!(receive(&mut peer).await, b"wrapped");
    assert_observations(&observations, 1);
    let mut malformed = frame(1, 7, b"accepted");
    *malformed.last_mut().unwrap() ^= 0xff;
    peer.write_all(&malformed).await.unwrap();
    assert_eq!(
        timeout(Duration::from_secs(3), peer.read(&mut [0]))
            .await
            .unwrap()
            .unwrap(),
        0
    );
    assert_observations(&observations, 1);
    let mut next = connect(addr).await;
    next.write_all(&frame(1, 7, b"denied")).await.unwrap();
    assert_eq!(receive(&mut next).await, b"denied");
    assert_observations(&observations, 2);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn verified_tls_uses_the_same_context_short_circuit_and_response_chain() {
    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let dir = std::env::temp_dir().join(format!("lynn-middleware-tls-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, signing_key.serialize_pem()).unwrap();
    let addr = tcp_addr();
    let (observations, task) = server(
        LynnServerConfigBuilder::new()
            .with_addr(addr)
            .unwrap()
            .with_tls(lynn_tcp::lynn_tls::TlsServerConfig::new(
                cert_path.to_str().unwrap(),
                key_path.to_str().unwrap(),
            )),
    )
    .await;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(cert.der().clone()).unwrap();
    let config = rustls::ClientConfig::builder_with_provider(Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13])
    .unwrap()
    .with_root_certificates(roots)
    .with_no_client_auth();
    let mut peer = tokio_rustls::TlsConnector::from(Arc::new(config))
        .connect("localhost".try_into().unwrap(), connect(addr).await)
        .await
        .unwrap();
    exchange(&mut peer).await;
    assert_observations(&observations, 2);
    peer.shutdown().await.unwrap();
    peer.read_to_end(&mut Vec::new()).await.unwrap();
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(all(feature = "client", feature = "kcp"))]
#[tokio::test]
async fn kcp_uses_the_same_context_short_circuit_and_response_chain() {
    use lynn_tcp::{
        lynn_client::{LynnClient, LynnClientConfigBuilder},
        lynn_kcp::KcpConfig,
    };

    let probe = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    let addr = probe.local_addr().unwrap();
    drop(probe);
    let (observations, task) = server(
        LynnServerConfigBuilder::new()
            .with_addr(addr)
            .unwrap()
            .with_kcp(KcpConfig::default()),
    )
    .await;
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_kcp(KcpConfig::default())
        .with_server_check_heart_interval(&300)
        .build();
    let mut peer = timeout(
        Duration::from_secs(5),
        LynnClient::new_with_config(config).await.start(),
    )
    .await
    .unwrap();
    assert!(peer.is_connected());
    for (payload, expected) in [("accepted", "wrapped"), ("denied", "denied")] {
        peer.send_data(HandlerResult::new_with_send_to_server(7, payload.into()).unwrap())
            .await
            .unwrap();
        let reply = timeout(Duration::from_secs(5), peer.get_receive_data())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reply.get_all_bytes().as_ref(), expected.as_bytes());
    }
    assert_observations(&observations, 2);
    drop(peer);
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
}
