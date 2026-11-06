use std::{error::Error, io, time::Duration};

use lynn_tcp::{
    lynn_client::{ConnectionState, LynnClient, LynnClientConfigBuilder},
    lynn_tcp_dependents::{HandlerResult, InputBufVOTrait},
};
use tokio::{net::TcpListener, sync::oneshot, time::timeout};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    timeout(Duration::from_secs(5), demonstrate_state()).await?
}

async fn demonstrate_state() -> Result<(), Box<dyn Error>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(listener.local_addr()?)?
        .with_reconnect_interval_secs(&0)
        .build();
    let (disconnect, disconnected) = oneshot::channel();
    let peer = tokio::spawn(async move {
        let (first, _) = listener.accept().await?;
        disconnected.await.map_err(io::Error::other)?;
        drop(first);
        let (second, _) = listener.accept().await?;
        let (mut read, mut write) = second.into_split();
        tokio::io::copy(&mut read, &mut write).await
    });
    let mut client = LynnClient::new_with_config(config).await.start().await;
    let mut state = client
        .subscribe_connection_state()
        .ok_or("startup failed")?;
    assert_eq!(
        *state.borrow_and_update(),
        ConnectionState::Connected { generation: 1 }
    );
    println!("initial connection: generation 1");
    disconnect.send(()).map_err(|_| "peer stopped")?;
    state
        .wait_for(|snapshot| *snapshot == ConnectionState::Connected { generation: 2 })
        .await?;

    let observer = tokio::spawn(async move {
        loop {
            if state.has_changed().is_err() {
                break;
            }
            let snapshot = *state.borrow_and_update();
            println!("{snapshot:?}");
            if state.changed().await.is_err() {
                println!("connection supervisor ended");
                break;
            }
        }
    });
    client
        .send_data(HandlerResult::new_with_send_to_server(
            1,
            "after reconnect".into(),
        )?)
        .await?;
    loop {
        let mut reply = client.get_receive_data().await.ok_or("reception ended")?;
        if reply.get_constructor_id() == Some(2) {
            continue;
        }
        assert_eq!(reply.get_all_bytes().as_ref(), b"after reconnect");
        break;
    }
    println!("received echo while observing state in another task");
    drop(client);
    observer.await?;
    peer.await??;
    Ok(())
}
