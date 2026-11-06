use super::*;
use crate::{
    const_config::DEFAULT_SYSTEM_CHANNEL_SIZE, infrastructure::tcp::session_io::SessionIo,
};
use std::{
    collections::HashSet,
    time::{Duration, SystemTime},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::RwLock,
    time::timeout,
};

#[tokio::test]
async fn full_queue_is_nonblocking_and_isolated_from_another_session() {
    let (_owner, tasks) = ServerTasks::new();
    let (completion, observer) = SessionObserver::new();
    let (transport, _blocked_peer) = tokio::io::duplex(1);
    let (read, write) = tokio::io::split(SessionIo::new(transport, completion));
    let (user, writer, removed) =
        LynnUser::new(Box::new(write), Arc::new(RwLock::new(SystemTime::now())), 0);
    let handle = SessionHandle::new(&user, observer, removed, tasks.clone(), (9177, 7719));
    assert_eq!(
        handle.try_send(1, "blocked".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    tokio::task::yield_now().await;
    for _ in 0..DEFAULT_SYSTEM_CHANNEL_SIZE {
        assert_eq!(
            handle.try_send(1, Bytes::new()).unwrap(),
            SessionSendOutcome::Enqueued
        );
    }
    assert_eq!(
        handle.try_send(1, Bytes::new()).unwrap(),
        SessionSendOutcome::Full
    );

    let (completion, observer) = SessionObserver::new();
    let (transport, mut healthy_peer) = tokio::io::duplex(64);
    let (healthy_read, write) = tokio::io::split(SessionIo::new(transport, completion));
    let (healthy, healthy_writer, removed) =
        LynnUser::new(Box::new(write), Arc::new(RwLock::new(SystemTime::now())), 0);
    let healthy_handle = SessionHandle::new(&healthy, observer, removed, tasks, (0xABCD, 0x1234));
    assert_eq!(
        healthy_handle.try_send(2, "ok".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    let mut response = [0; 17];
    timeout(
        Duration::from_secs(1),
        healthy_peer.read_exact(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(&response[..2], &0xABCD_u16.to_le_bytes());
    assert_eq!(&response[13..15], b"ok");
    assert_eq!(&response[15..], &0x1234_u16.to_le_bytes());
    assert_ne!(handle.id(), healthy_handle.id());
    assert_eq!(
        HashSet::from([
            handle.id().clone(),
            handle.clone().id().clone(),
            healthy_handle.id().clone()
        ])
        .len(),
        2
    );
    drop(user);
    assert_eq!(
        handle.try_send(1, Bytes::new()).unwrap(),
        SessionSendOutcome::Closed
    );
    writer.abort();
    assert!(writer.await.unwrap_err().is_cancelled());
    assert!(!handle.observer().is_closed());
    drop(read);
    timeout(Duration::from_secs(1), handle.observer().closed())
        .await
        .unwrap();
    drop(healthy);
    healthy_writer.await.unwrap();
    drop(healthy_read);
    timeout(Duration::from_secs(1), healthy_handle.observer().closed())
        .await
        .unwrap();
}

#[tokio::test]
async fn owner_cancellation_rejects_enqueue_before_transport_teardown() {
    let (owner, tasks) = ServerTasks::new();
    let (completion, observer) = SessionObserver::new();
    let (transport, _peer) = tokio::io::duplex(64);
    let (read, write) = tokio::io::split(SessionIo::new(transport, completion));
    let (user, writer, removed) =
        LynnUser::new(Box::new(write), Arc::new(RwLock::new(SystemTime::now())), 0);
    let handle = SessionHandle::new(&user, observer, removed, tasks, (9177, 7719));
    drop(owner);
    assert_eq!(
        handle.try_send(1, Bytes::new()).unwrap(),
        SessionSendOutcome::Closed
    );
    assert!(!handle.observer().is_closed());
    drop(user);
    drop(read);
    writer.await.unwrap();
    timeout(Duration::from_secs(1), handle.observer().closed())
        .await
        .unwrap();
}

#[cfg(feature = "kcp")]
#[tokio::test]
async fn kcp_observer_waits_for_the_aborted_driver_to_release_its_socket() {
    use crate::infrastructure::{
        kcp::{KcpConfig, stream::KcpStream},
        tcp::stream::{LynnStream, split_transport},
    };

    for poll_driver in [false, true] {
        let peer = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let addr = peer.local_addr().unwrap();
        let socket = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
        socket.connect(addr).await.unwrap();
        let weak_socket = Arc::downgrade(&socket);
        let (stream, _ready) =
            KcpStream::start(socket, addr, 123, KcpConfig::default(), None).unwrap();
        let (read, write, observer) = split_transport(LynnStream::Kcp(stream));
        if poll_driver {
            tokio::task::yield_now().await;
        }
        drop(read);
        drop(write);
        assert!(!observer.is_closed());
        timeout(Duration::from_secs(1), observer.closed())
            .await
            .unwrap();
        assert!(observer.is_closed());
        assert!(weak_socket.upgrade().is_none());
    }
}

#[cfg(feature = "kcp")]
#[tokio::test]
async fn kcp_ordered_eof_and_same_address_reconnection_keep_original_identity() {
    use crate::{
        application::server::{lynn_server::LynnServer, server_config::LynnServerConfigBuilder},
        domain::handler::request_context::RequestContext,
        infrastructure::kcp::{KcpConfig, stream::KcpStream},
    };
    let (registered, mut registrations) = tokio::sync::mpsc::unbounded_channel();
    let config = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:0")
        .unwrap()
        .with_kcp(KcpConfig::default())
        .with_connection_rate_limit(&0)
        .with_max_connections_per_ip(&0)
        .build();
    let server =
        LynnServer::new_with_config(config)
            .await
            .add_router(1, move |request: RequestContext| {
                let registered = registered.clone();
                async move {
                    registered.send(request.session().clone()).unwrap();
                    HandlerResult::new_with_send(1, "ok".into(), vec![request.peer_addr()]).unwrap()
                }
            });
    let ready = server.startup_handle();
    let server_task = tokio::spawn(server.start());
    let addr = timeout(Duration::from_secs(3), ready.wait())
        .await
        .unwrap()
        .unwrap();
    let socket = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
    socket.connect(addr).await.unwrap();
    let mut previous: Option<SessionHandle> = None;
    for conversation in [42, 43] {
        let (mut client, ready) = KcpStream::start(
            socket.clone(),
            addr,
            conversation,
            KcpConfig::default(),
            None,
        )
        .unwrap();
        timeout(Duration::from_secs(3), ready)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let request = HandlerResult::new_with_send(1, Bytes::new(), vec![])
            .unwrap()
            .get_response_data()
            .unwrap();
        client.write_all(&request).await.unwrap();
        client.flush().await.unwrap();
        let handle = timeout(Duration::from_secs(3), registrations.recv())
            .await
            .unwrap()
            .unwrap();
        let mut response = [0; 17];
        timeout(Duration::from_secs(3), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response[13..15], b"ok");
        if let Some(old) = &previous {
            assert_ne!(old.id(), handle.id());
            assert!(old.observer().is_closed());
            assert_eq!(
                old.try_send(2, "private old".into()).unwrap(),
                SessionSendOutcome::Closed
            );
        }
        assert_eq!(
            handle.try_send(3, "new".into()).unwrap(),
            SessionSendOutcome::Enqueued
        );
        let mut pushed = [0; 18];
        timeout(Duration::from_secs(3), client.read_exact(&mut pushed))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&pushed[11..13], &3_u16.to_le_bytes());
        assert_eq!(&pushed[13..16], b"new");
        timeout(Duration::from_secs(3), client.shutdown())
            .await
            .unwrap()
            .unwrap();
        let mut remaining = Vec::new();
        timeout(Duration::from_secs(3), client.read_to_end(&mut remaining))
            .await
            .unwrap()
            .unwrap();
        assert!(remaining.is_empty());
        timeout(Duration::from_secs(3), handle.observer().closed())
            .await
            .unwrap();
        let mut driver = client.completion();
        drop(client);
        timeout(Duration::from_secs(3), async {
            while driver.changed().await.is_ok() {}
        })
        .await
        .unwrap();
        previous = Some(handle);
    }
    server_task.abort();
    assert!(server_task.await.unwrap_err().is_cancelled());
}
