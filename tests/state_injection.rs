//! Integration tests: global state (`AppState<T>`) injection into server handlers.
//!
//! Verifies that `LynnServer::with_state` values reach handler parameters,
//! that multiple state types coexist, that registration order relative to
//! `add_router` does not matter, and that a missing state cannot take down
//! the reactor (handler panic isolation).

use std::time::Duration;

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_state::{AppState, StateRegistry},
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
    tokio::{net::TcpStream, time::timeout},
};

/// Static constants used to satisfy the `&'a` config builder lifetimes.
static MAX_CONN: usize = 1000;

// ── fake service state ──────────────────────────────────────────────────

#[derive(Clone)]
struct UserRepo {
    prefix: String,
}

impl UserRepo {
    fn find_user(&self, id: u64) -> String {
        format!("{}{id}", self.prefix)
    }
}

struct AppConfig {
    greeting: String,
}

// ── handlers ────────────────────────────────────────────────────────────

async fn find_user(repo: AppState<UserRepo>, input: InputBufVO) -> HandlerResult {
    let id: u64 = String::from_utf8_lossy(&input.get_all_bytes())
        .parse()
        .unwrap_or(0);
    let name = repo.find_user(id);
    HandlerResult::new_with_send(1, name.into(), vec![input.get_input_addr().unwrap()])
}

async fn greeting(config: AppState<AppConfig>, input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(
        2,
        config.greeting.clone().into(),
        vec![input.get_input_addr().unwrap()],
    )
}

async fn both_states(
    repo: AppState<UserRepo>,
    config: AppState<AppConfig>,
    input: InputBufVO,
) -> HandlerResult {
    HandlerResult::new_with_send(
        3,
        format!("{}|{}", config.greeting, repo.find_user(7)).into(),
        vec![input.get_input_addr().unwrap()],
    )
}

async fn needs_missing_state(_db: AppState<StateRegistryHolder>) -> HandlerResult {
    HandlerResult::new_without_send()
}

/// A type that is never registered on purpose.
struct StateRegistryHolder;

async fn still_alive(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(9, "alive".into(), vec![input.get_input_addr().unwrap()])
}

// ── helpers ─────────────────────────────────────────────────────────────

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .expect("bind probe")
        .local_addr()
        .unwrap()
        .port()
}

async fn spawn_server(
    port: u16,
    build: impl FnOnce(LynnServer<'static>) -> LynnServer<'static> + Send + 'static,
) {
    let addr = format!("127.0.0.1:{port}");
    tokio::spawn(async move {
        let server = LynnServer::new_with_config(
            LynnServerConfigBuilder::new()
                .with_addr(&addr)
                .expect("addr")
                .with_server_max_connections(Some(&MAX_CONN))
                .build(),
        )
        .await;
        build(server).start().await;
    });
    // Wait until the port accepts connections.
    for _ in 0..120 {
        if TcpStream::connect(format!("127.0.0.1:{port}"))
            .await
            .is_ok()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("server never became reachable");
}

async fn ask(port: u16, method_id: u16, payload: &str) -> Option<String> {
    let mut raw = TcpStream::connect(format!("127.0.0.1:{port}"))
        .await
        .expect("connect");
    let msg_len = (1 + 2 + payload.len() + 2) as u64;
    let mut frame = Vec::new();
    frame.extend_from_slice(&9177u16.to_le_bytes());
    frame.extend_from_slice(&msg_len.to_le_bytes());
    frame.push(1u8);
    frame.extend_from_slice(&method_id.to_le_bytes());
    frame.extend_from_slice(payload.as_bytes());
    frame.extend_from_slice(&7719u16.to_le_bytes());
    use lynn_tcp::tokio::io::AsyncWriteExt;
    raw.write_all(&frame).await.expect("write frame");

    let mut head = [0u8; 10];
    let read = timeout(Duration::from_secs(3), async {
        use lynn_tcp::tokio::io::AsyncReadExt;
        raw.read_exact(&mut head).await
    })
    .await;
    match read {
        Err(_) => return None, // no response in time
        Ok(Err(_)) => return None,
        Ok(Ok(_)) => {},
    }
    let body_len = u64::from_le_bytes(head[2..10].try_into().unwrap()) as usize;
    let mut rest = vec![0u8; body_len];
    timeout(Duration::from_secs(3), async {
        use lynn_tcp::tokio::io::AsyncReadExt;
        raw.read_exact(&mut rest).await
    })
    .await
    .expect("read body timeout")
    .expect("read body");
    Some(String::from_utf8_lossy(&rest[3..body_len - 2]).to_string())
}

// ── tests ───────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_is_injected_into_handler_params() {
    let port = free_port();
    spawn_server(port, |s| {
        s.with_state(UserRepo {
            prefix: "user-".to_string(),
        })
        .add_router(1, find_user)
    })
    .await;

    let body = ask(port, 1, "42").await.expect("response");
    assert_eq!(body, "user-42");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn multiple_state_types_coexist() {
    let port = free_port();
    spawn_server(port, |s| {
        s.with_state(UserRepo {
            prefix: "u".to_string(),
        })
        .with_state(AppConfig {
            greeting: "hello".to_string(),
        })
        .add_router(2, greeting)
        .add_router(3, both_states)
    })
    .await;

    assert_eq!(ask(port, 2, "").await.expect("greeting"), "hello");
    assert_eq!(
        ask(port, 3, "").await.expect("both"),
        "hello|u7",
        "both AppState<T> params must resolve independently"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_registered_after_add_router_still_resolves() {
    let port = free_port();
    spawn_server(port, |s| {
        // add_router first, with_state later — extraction is per request.
        s.add_router(1, find_user).with_state(UserRepo {
            prefix: "late-".to_string(),
        })
    })
    .await;

    assert_eq!(ask(port, 1, "1").await.expect("response"), "late-1");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn missing_state_panics_are_isolated_and_server_survives() {
    let port = free_port();
    spawn_server(port, |s| {
        // `needs_missing_state` requires a type that is never registered;
        // `still_alive` uses no state and must keep working afterwards.
        s.add_router(8, needs_missing_state)
            .add_router(9, still_alive)
    })
    .await;

    // Triggers the handler panic (caught by the reactor worker): no response.
    let boom = ask(port, 8, "boom").await;
    assert_eq!(
        boom, None,
        "a panicking handler must not produce a response"
    );

    // The server must still respond normally after the panic.
    assert_eq!(
        ask(port, 9, "").await.expect("server must survive"),
        "alive"
    );
}

#[test]
fn state_registry_is_public_api() {
    let registry = StateRegistry::new();
    registry.set(UserRepo { prefix: "x".into() });
    assert!(registry.contains::<UserRepo>());
    assert_eq!(registry.get::<UserRepo>().unwrap().prefix, "x");
}
