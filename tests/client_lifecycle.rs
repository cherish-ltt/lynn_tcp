#![cfg(feature = "client")]

use std::time::Duration;

use lynn_tcp::{
    lynn_client::{ConnectionState, LynnClient, LynnClientConfigBuilder},
    lynn_tcp_dependents::HandlerResult,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::timeout,
};

async fn assert_closed(peer: &mut TcpStream) {
    let mut byte = [0];
    assert_eq!(
        timeout(Duration::from_secs(2), peer.read(&mut byte))
            .await
            .expect("dropped client must close the socket promptly")
            .expect("read EOF"),
        0,
        "dropped client must not keep sending heartbeats"
    );
}

#[tokio::test]
async fn drop_cancels_backpressured_write() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(listener.local_addr().unwrap())
        .unwrap()
        .with_server_single_channel_size(&1)
        .build();
    let mut client = LynnClient::new_with_config(config).await.start().await;
    let sender = client.get_sender().await.unwrap();
    let (_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let payload = lynn_tcp::bytes::Bytes::from(vec![0; 4 * 1024 * 1024]);
    timeout(Duration::from_secs(3), async {
        loop {
            let message = HandlerResult::new_with_send_to_server(1, payload.clone())
                .expect("payload fits the protocol limit");
            if timeout(Duration::from_millis(100), sender.send(message))
                .await
                .is_err()
            {
                break;
            }
        }
    })
    .await
    .expect("non-reading peer must backpressure writes");

    drop(client);

    timeout(Duration::from_secs(2), sender.closed())
        .await
        .expect("blocked write must be cancelled with the owner");
}

#[tokio::test]
async fn drop_cancels_backpressured_read() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(listener.local_addr().unwrap())
        .unwrap()
        .with_server_single_channel_size(&1)
        .build();
    let mut client = LynnClient::new_with_config(config).await.start().await;
    let sender = client.get_sender().await.unwrap();
    let (mut peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut heartbeat = [0; 15];
    timeout(Duration::from_secs(2), peer.read_exact(&mut heartbeat))
        .await
        .unwrap()
        .unwrap();
    let mut frame = Vec::new();
    frame.extend_from_slice(&9177u16.to_le_bytes());
    frame.extend_from_slice(&13u64.to_le_bytes());
    frame.extend_from_slice(&[1, 1, 0]);
    frame.extend_from_slice(b"response");
    frame.extend_from_slice(&7719u16.to_le_bytes());
    timeout(Duration::from_secs(2), peer.write_all(&frame.repeat(8)))
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    drop(client);

    timeout(Duration::from_secs(2), sender.closed())
        .await
        .expect("blocked receive channel must be cancelled with the owner");
    assert_closed(&mut peer).await;
}

#[tokio::test]
async fn restarting_client_closes_previous_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut client = LynnClient::new_with_addr(listener.local_addr().unwrap())
        .await
        .start()
        .await;
    let old_sender = client.get_sender().await.unwrap();
    let mut old_state = client.subscribe_connection_state().unwrap();
    old_state.borrow_and_update();
    let (mut old_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut heartbeat = [0; 15];
    timeout(Duration::from_secs(2), old_peer.read_exact(&mut heartbeat))
        .await
        .unwrap()
        .unwrap();

    let client = client.start().await;

    timeout(Duration::from_secs(2), old_sender.closed())
        .await
        .expect("starting again must not detach the previous task group");
    assert_closed(&mut old_peer).await;
    let (_new_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    assert!(client.is_connected());
    assert!(
        timeout(Duration::from_secs(2), old_state.changed())
            .await
            .unwrap()
            .is_err()
    );
    let state = client.subscribe_connection_state().unwrap();
    assert_eq!(
        *state.borrow(),
        ConnectionState::Connected { generation: 1 }
    );
}

#[tokio::test]
async fn cancelling_restart_ends_the_previous_subscription() {
    use std::{future::Future, task::Poll};

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = LynnClient::new_with_addr(listener.local_addr().unwrap())
        .await
        .start()
        .await;
    let mut state = client.subscribe_connection_state().unwrap();
    state.borrow_and_update();
    let (mut peer, _) = listener.accept().await.unwrap();
    timeout(Duration::from_secs(2), peer.read_exact(&mut [0; 15]))
        .await
        .unwrap()
        .unwrap();
    let mut restart = Box::pin(client.start());
    assert!(std::future::poll_fn(|cx| Poll::Ready(restart.as_mut().poll(cx).is_pending())).await);
    drop(restart);

    assert!(
        timeout(Duration::from_secs(2), state.changed())
            .await
            .unwrap()
            .is_err()
    );
    assert_closed(&mut peer).await;
    assert!(
        timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}

#[cfg(feature = "tls")]
#[tokio::test]
async fn cancelling_start_closes_pending_tls_handshake() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(listener.local_addr().unwrap())
        .unwrap()
        .with_connect_timeout_secs(&60)
        .with_tls(
            lynn_tcp::lynn_tls::TlsClientConfigBuilder::new()
                .with_danger_accept_invalid_certs()
                .build(),
        )
        .build();
    let start =
        tokio::spawn(async move { LynnClient::new_with_config(config).await.start().await });
    let (mut peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut hello = [0; 4096];
    assert!(
        timeout(Duration::from_secs(2), peer.read(&mut hello))
            .await
            .unwrap()
            .unwrap()
            > 0
    );

    start.abort();
    assert!(matches!(start.await, Err(error) if error.is_cancelled()));

    timeout(Duration::from_secs(2), peer.read_to_end(&mut Vec::new()))
        .await
        .expect("cancelled start must close the in-progress TLS socket")
        .unwrap();
    assert!(
        timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn drop_closes_idle_connection_even_with_cloned_sender() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    for _ in 0..5 {
        let config = LynnClientConfigBuilder::new()
            .with_server_addr(listener.local_addr().unwrap())
            .unwrap()
            .with_server_check_heart_interval(&1)
            .build();
        let mut client = LynnClient::new_with_config(config).await.start().await;
        let sender = client.get_sender().await.unwrap();
        let mut state = client.subscribe_connection_state().unwrap();
        state.borrow_and_update();
        let (mut peer, _) = timeout(Duration::from_secs(2), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut heartbeat = [0; 15];
        timeout(Duration::from_secs(2), peer.read_exact(&mut heartbeat))
            .await
            .unwrap()
            .unwrap();

        drop(client);

        timeout(Duration::from_secs(2), sender.closed())
            .await
            .expect("sender must not extend the owner lifetime");
        assert!(
            timeout(Duration::from_secs(2), state.changed())
                .await
                .unwrap()
                .is_err()
        );
        assert_eq!(
            *state.borrow(),
            ConnectionState::Connected { generation: 1 }
        );
        assert_closed(&mut peer).await;
        assert!(
            sender
                .send(
                    HandlerResult::new_with_send_to_server(1, "late".into())
                        .expect("payload fits the protocol limit")
                )
                .await
                .is_err()
        );
    }
    assert!(
        timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn drop_cancels_reconnect_delay() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let config = LynnClientConfigBuilder::new()
        .with_server_addr(addr)
        .unwrap()
        .with_reconnect_interval_secs(&60)
        .build();
    let mut client = LynnClient::new_with_config(config).await.start().await;
    let sender = client.get_sender().await.unwrap();
    let mut state = client.subscribe_connection_state().unwrap();
    let (peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .unwrap()
        .unwrap();
    drop(listener);
    drop(peer);
    timeout(
        Duration::from_secs(2),
        state.wait_for(|value| *value == ConnectionState::Reconnecting { generation: 1 }),
    )
    .await
    .unwrap()
    .unwrap();

    drop(client);

    timeout(Duration::from_secs(2), sender.closed())
        .await
        .expect("reconnect delay must not keep the write receiver alive");
    assert!(state.changed().await.is_err());
}
