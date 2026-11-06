//! Run with `cargo run --example middleware_example`.
//! The plaintext loopback credential below is public demo data, not a secret.
//! Use a verified encrypted transport for real credentials (see `tls_example`).

use std::{net::SocketAddr, time::Duration};

use lynn_tcp::{
    LynnError,
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_middleware::{MiddlewareChain, MiddlewareRequest, Next, RequestContext},
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
};
use tokio::time::{Instant, timeout};

struct DemoPolicy {
    marker: &'static [u8],
}
struct Principal(&'static str);

async fn authorize(mut request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
    let policy = request
        .app_state::<DemoPolicy>()
        .ok_or_else(|| LynnError::Config("demo policy missing".into()))?;
    if !request.payload().starts_with(policy.marker) {
        return HandlerResult::new_with_send(
            request.method_id(),
            "DENIED".into(),
            vec![request.peer_addr()],
        );
    }
    request.insert(Principal("demo-user"));
    next.run(request).await
}

async fn deadline(request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
    timeout(Duration::from_millis(100), next.run(request))
        .await
        .map_err(|_| LynnError::Timeout("demo request deadline".into()))?
}

async fn map_errors(request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
    let (method, peer) = (request.method_id(), request.peer_addr());
    match next.run(request).await {
        Ok(result) => Ok(result),
        Err(error) => {
            let message = if matches!(error, LynnError::Timeout(_)) {
                "DEADLINE"
            } else {
                "UNAVAILABLE"
            };
            tracing::warn!(method, response_code = message, "request error mapped");
            HandlerResult::new_with_send(method, message.into(), vec![peer])
        },
    }
}

async fn observe(request: MiddlewareRequest, next: Next) -> lynn_tcp::Result<HandlerResult> {
    let method = request.method_id();
    let started = Instant::now();
    let result = next.run(request).await;
    let response_bytes = result
        .as_ref()
        .ok()
        .and_then(HandlerResult::response_view)
        .map(|view| view.payload().len());
    tracing::info!(
        method,
        elapsed_us = started.elapsed().as_micros(),
        returned_ok = result.is_ok(),
        response_bytes,
        "request chain returned"
    );
    // A mapped business rejection is Ok here; this event is not delivery or audit proof.
    result
}

async fn protected(context: RequestContext, input: InputBufVO) -> HandlerResult {
    let Some(principal) = context.get::<Principal>() else {
        return HandlerResult::new_with_send(
            context.method_id(),
            "DENIED".into(),
            vec![context.peer_addr()],
        )
        .expect("fixed response fits");
    };
    if input.get_all_bytes().ends_with(b"slow") {
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    HandlerResult::new_with_send(
        context.method_id(),
        format!("WELCOME {}", principal.0).into(),
        vec![context.peer_addr()],
    )
    .expect("fixed response fits")
}

async fn exchange(addr: SocketAddr) -> Result<(), Box<dyn std::error::Error>> {
    let attempts = 1;
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)?
        .with_reconnect_max_attempts(&attempts)
        .build();
    let mut client = LynnClient::new_with_config(config).await.start().await;
    if !client.is_connected() {
        return Err("demo server did not become ready".into());
    }
    for (payload, expected) in [
        ("public-demo:hello", "WELCOME demo-user"),
        ("invalid", "DENIED"),
        ("public-demo:slow", "DEADLINE"),
        ("public-demo:again", "WELCOME demo-user"),
    ] {
        client
            .send_data(HandlerResult::new_with_send_to_server(1, payload.into())?)
            .await?;
        let response = timeout(Duration::from_secs(2), client.get_receive_data())
            .await?
            .ok_or("demo connection ended before its response")?;
        assert_eq!(response.get_all_bytes(), expected.as_bytes());
        println!("response: {expected}");
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();
    let config = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:0")?
        .build();
    let app = LynnServer::new_with_config(config)
        .await
        .with_state(DemoPolicy {
            marker: b"public-demo:",
        })
        .with_middleware(observe)
        .with_middleware(map_errors)
        .add_router_with_middleware(
            1,
            protected,
            MiddlewareChain::new().with(deadline).with(authorize),
        );
    let ready = app.startup_handle();
    let server = tokio::spawn(app.start());
    let result = match ready.wait().await {
        Ok(addr) => exchange(addr).await,
        Err(error) => Err(error.into()),
    };
    server.abort();
    match server.await {
        Err(error) if error.is_cancelled() => {},
        Err(error) => return Err(error.into()),
        Ok(Err(error)) => return Err(error.into()),
        Ok(Ok(())) => {},
    }
    result
}
