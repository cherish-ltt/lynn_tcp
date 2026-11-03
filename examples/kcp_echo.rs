//! Run: cargo run --example kcp_echo --features kcp
use lynn_tcp::{
    lynn_client::{LynnClient, LynnClientConfigBuilder},
    lynn_kcp::KcpConfig,
    lynn_server::{LynnServer, LynnServerConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVO, InputBufVOTrait},
};

async fn echo(input: InputBufVO) -> HandlerResult {
    HandlerResult::new_with_send(
        1,
        input.get_all_bytes().into(),
        vec![input.get_input_addr().unwrap()],
    )
    .expect("received payload fits the frame limit")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr = "127.0.0.1:9177";
    let config = LynnServerConfigBuilder::new()
        .with_addr(addr)?
        .with_kcp(KcpConfig::default())
        .build();
    let server = tokio::spawn(
        LynnServer::new_with_config(config)
            .await
            .add_router(1, echo)
            .start(),
    );
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)?
        .with_kcp(KcpConfig::default())
        .build();
    let mut client = LynnClient::new_with_config(config).await.start().await;
    let result = async {
        if !client.is_connected() {
            return Err("KCP connection failed".into());
        }
        client
            .send_data(HandlerResult::new_with_send_to_server(
                1,
                "hello KCP".into(),
            )?)
            .await?;
        let response =
            tokio::time::timeout(std::time::Duration::from_secs(5), client.get_receive_data())
                .await?
                .ok_or("KCP disconnected")?;
        assert_eq!(response.get_all_bytes().as_ref(), b"hello KCP");
        println!("KCP echo: hello KCP");
        Ok::<(), Box<dyn std::error::Error>>(())
    }
    .await;
    drop(client);
    server.abort();
    let _ = server.await;
    result
}
