//! Integration tests with custom message header/tail marks.
//!
//! Lives in its own test binary because the server-side marks are process-global
//! (`OnceLock` in `const_config`), initialized by the first server that starts.

use std::time::Duration;

use lynn_tcp::{
    bytes::Bytes,
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_server::{ClientsContext, LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
    tokio::{net::TcpStream, time::timeout},
};

static CUSTOM_HEADER: u16 = 0xABCD;
static CUSTOM_TAIL: u16 = 0xEF01;

async fn echo(input_buf_vo: InputBufVO, _clients: ClientsContext) -> HandlerResult {
    eprintln!(">>> custom echo handler fired");
    let payload = String::from_utf8_lossy(&input_buf_vo.get_all_bytes()).to_string();
    let addr = input_buf_vo.get_input_addr().unwrap();
    eprintln!(">>> replying to {addr}");
    HandlerResult::new_with_send(1, format!("Custom: {payload}").into(), vec![addr])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_marks_roundtrip() {
    lynn_tcp::tracing_subscriber::fmt::try_init().ok();
    let addr = "127.0.0.1:9321".to_string();
    let server_addr = addr.clone();

    tokio::spawn(async move {
        let config = LynnServerConfigBuilder::new()
            .with_addr(server_addr)
            .expect("addr")
            .with_message_header_mark(&CUSTOM_HEADER)
            .with_message_tail_mark(&CUSTOM_TAIL)
            .build();
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo)
            .start()
            .await;
    });

    // Wait for the listener with raw probes (they carry no protocol data).
    let mut ready = false;
    for _ in 0..120 {
        if TcpStream::connect(&addr).await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(ready, "custom-mark server never became reachable");

    let mut client = LynnClient::new_with_config(
        LynnClientConfigBuilder::new()
            .with_server_addr(&addr)
            .expect("addr")
            .with_message_header_mark(&CUSTOM_HEADER)
            .with_message_tail_mark(&CUSTOM_TAIL)
            .build(),
    )
    .await
    .start()
    .await;

    client
        .send_data(HandlerResult::new_with_send_to_server(
            1,
            Bytes::from("marked".to_owned()),
        ))
        .await
        .expect("send failed");

    let mut resp = timeout(Duration::from_secs(5), client.get_receive_data())
        .await
        .expect("timed out waiting for response")
        .expect("connection closed");
    assert_eq!(resp.get_method_id(), Some(1));
    assert_eq!(String::from_utf8_lossy(&resp.get_all_bytes()), "Custom: marked");
}
