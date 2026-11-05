use std::{sync::Arc, time::SystemTime};

use bytes::{Bytes, BytesMut};
use dashmap::DashMap;
use tokio::{io::AsyncReadExt, sync::RwLock};

use super::{broadcast::*, server_tasks::ServerTasks};
use crate::{const_config::DEFAULT_SYSTEM_CHANNEL_SIZE, domain::model::lynn_user::LynnUser};

#[tokio::test]
async fn full_and_closed_targets_do_not_block_healthy_target() {
    let clients = Arc::new(DashMap::new());
    let (_owner, tasks) = ServerTasks::new();
    let handle = BroadcastHandle::new(
        &clients,
        Arc::new(std::sync::atomic::AtomicBool::new(true)),
        tasks,
        (0x1234, 0x5678),
    );
    let (blocked, _blocked_peer) = tokio::io::duplex(1);
    let (full, blocked_writer, _) = LynnUser::new(
        Box::new(blocked),
        Arc::new(RwLock::new(SystemTime::now())),
        0,
    );
    full.send_response(&BytesMut::from(&b"blocked"[..])).await;
    tokio::task::yield_now().await;
    for _ in 0..DEFAULT_SYSTEM_CHANNEL_SIZE {
        full.send_response(&BytesMut::from(&b"queued"[..])).await;
    }
    let (closed, closed_writer, _) = LynnUser::new(
        Box::new(tokio::io::sink()),
        Arc::new(RwLock::new(SystemTime::now())),
        0,
    );
    closed_writer.abort();
    assert!(closed_writer.await.unwrap_err().is_cancelled());
    let (healthy, mut peer) = tokio::io::duplex(128);
    let (good, good_writer, _) = LynnUser::new(
        Box::new(healthy),
        Arc::new(RwLock::new(SystemTime::now())),
        0,
    );
    clients.insert("127.0.0.1:1".parse().unwrap(), full);
    clients.insert("127.0.0.1:2".parse().unwrap(), closed);
    clients.insert("127.0.0.1:3".parse().unwrap(), good);
    assert_eq!(
        handle.broadcast(42, Bytes::from_static(b"ok")).unwrap(),
        BroadcastReport {
            enqueued: 1,
            full: 1,
            closed: 1
        }
    );
    let mut response = [0; 17];
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        peer.read_exact(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(&response[..2], &0x1234u16.to_le_bytes());
    assert_eq!(&response[11..13], &42u16.to_le_bytes());
    assert_eq!(&response[13..15], b"ok");
    assert_eq!(&response[15..], &0x5678u16.to_le_bytes());
    // No map guard or pending enqueue survives the call.
    clients.clear();
    blocked_writer.abort();
    good_writer.abort();
    let _ = blocked_writer.await;
    let _ = good_writer.await;
}

#[tokio::test]
async fn snapshot_sender_cannot_redirect_to_replacement_at_same_address() {
    let clients = Arc::new(DashMap::new());
    let addr: std::net::SocketAddr = "127.0.0.1:1".parse().unwrap();
    let (old, old_writer, _) = LynnUser::new(
        Box::new(tokio::io::sink()),
        Arc::new(RwLock::new(SystemTime::now())),
        0,
    );
    clients.insert(addr, old);
    let snapshot = clients.get(&addr).unwrap().response_sender();
    let (new, new_writer, _) = LynnUser::new(
        Box::new(tokio::io::sink()),
        Arc::new(RwLock::new(SystemTime::now())),
        0,
    );
    clients.insert(addr, new);
    old_writer.abort();
    let _ = old_writer.await;
    assert!(snapshot.is_closed());
    assert!(!clients.get(&addr).unwrap().response_sender().is_closed());
    new_writer.abort();
    let _ = new_writer.await;
}
