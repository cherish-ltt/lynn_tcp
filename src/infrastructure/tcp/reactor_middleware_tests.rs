use super::*;
use crate::{
    const_config::DEFAULT_SYSTEM_CHANNEL_SIZE,
    domain::{
        handler::{
            handler_system::{ClientsContext, HandlerContext},
            middleware::MiddlewareChain,
            request_context::RequestContext,
        },
        model::{handler_result::HandlerResult, input_buf_vo::InputBufVO, lynn_user::LynnUser},
    },
};
use bytes::BytesMut;
use dashmap::DashMap;
use tokio::{io::AsyncReadExt, time::timeout};

async fn blocked_response_keeps_permit(cancel_owner: bool) {
    let (owner, tasks) = ServerTasks::new();
    let manager = EventManager::new(tasks);
    let clients = Arc::new(DashMap::new());
    let addr: SocketAddr = "127.0.0.1:1234".parse().unwrap();
    let (transport, mut peer) = tokio::io::duplex(1);
    let (user, writer, _) = LynnUser::new(
        Box::new(transport),
        Arc::new(RwLock::new(SystemTime::now())),
        0,
    );
    timeout(Duration::from_secs(3), async {
        for _ in 0..=DEFAULT_SYSTEM_CHANNEL_SIZE {
            user.send_response(&BytesMut::from(&b"xx"[..])).await;
        }
    })
    .await
    .unwrap();
    clients.insert(addr, user);
    let router = Arc::new(LynnRouter::new());
    let after = Arc::new(Semaphore::new(0));
    let observed = after.clone();
    router.add_router_with_middleware(
        1,
        move || async move {
            HandlerResult::new_with_send(1, "response".into(), vec![addr]).unwrap()
        },
        MiddlewareChain::new().with(move |request, next| {
            let observed = observed.clone();
            async move {
                let result = next.run(request).await;
                observed.add_permits(1);
                result
            }
        }),
    );
    let permits = Arc::new(Semaphore::new(1));
    let (tx, _rx) = mpsc::channel(1);
    manager.run(
        clients.clone(),
        &1,
        9177,
        7719,
        router.clone(),
        manager.get_global_queue(),
        tx,
        &1,
        Arc::new(StreamAcceptor::Plain),
        0,
        0,
    );
    manager
        .get_global_queue()
        .push(ReactorEvent::crate_execute_task_event((
            router.get_handler_by_method_id(&1).unwrap(),
            HandlerContext::new(
                InputBufVO::new([1, 1, 0].as_slice().into(), addr),
                ClientsContext::new_none(),
                Arc::new(StateRegistry::new()),
                RequestContext::new(
                    1,
                    addr,
                    crate::domain::model::session::SessionHandle::closed_for_test(),
                ),
            ),
            clients.clone(),
            permits.clone().acquire_owned().await.unwrap(),
        )));
    timeout(Duration::from_secs(3), after.acquire())
        .await
        .unwrap()
        .unwrap()
        .forget();
    assert!(
        timeout(Duration::from_millis(20), permits.acquire())
            .await
            .is_err()
    );
    if cancel_owner {
        drop(owner);
    } else {
        let mut queued = vec![0; 2 * (DEFAULT_SYSTEM_CHANNEL_SIZE + 1)];
        timeout(Duration::from_secs(3), peer.read_exact(&mut queued))
            .await
            .unwrap()
            .unwrap();
        assert!(queued.iter().all(|&byte| byte == b'x'));
    }
    let released = timeout(Duration::from_secs(3), permits.acquire())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(permits.available_permits(), 0);
    drop(released);
    assert_eq!(permits.available_permits(), 1);
    writer.abort();
    assert!(writer.await.unwrap_err().is_cancelled());
    clients.clear();
}

#[tokio::test]
async fn middleware_permit_covers_final_response_enqueue() {
    blocked_response_keeps_permit(false).await;
}

#[tokio::test]
async fn owner_cancels_middleware_result_blocked_on_full_response_queue() {
    blocked_response_keeps_permit(true).await;
}

#[tokio::test]
async fn delayed_request_keeps_original_session_after_same_address_replacement()
-> Result<(), &'static str> {
    use crate::{
        application::server::server_common::input_dto_build,
        domain::model::session::{SessionHandle, SessionObserver, SessionSendOutcome},
    };
    let (_owner, tasks) = ServerTasks::new();
    let addr: SocketAddr = "127.0.0.1:1234".parse().unwrap();
    let clients = Arc::new(DashMap::new());
    let (old_completion, old_observer) = SessionObserver::new();
    let (old_user, old_writer, old_removed) = LynnUser::new(
        Box::new(tokio::io::sink()),
        Arc::new(RwLock::new(SystemTime::now())),
        0,
    );
    let old_handle = SessionHandle::new(
        &old_user,
        old_observer,
        old_removed,
        tasks.clone(),
        (9177, 7719),
    );
    clients.insert(addr, old_user);
    let (seen, mut observed) = mpsc::unbounded_channel();
    let router = LynnRouter::new();
    let middleware_id = old_handle.id().clone();
    router.add_router_with_middleware(
        1,
        move |context: RequestContext| {
            let seen = seen.clone();
            async move {
                seen.send(context.session().clone()).unwrap();
                HandlerResult::new_without_send()
            }
        },
        MiddlewareChain::new().with(move |request, next| {
            assert_eq!(request.session().id(), &middleware_id);
            next.run(request)
        }),
    );
    let queue = Arc::new(Injector::new());
    input_dto_build(
        addr,
        1,
        InputBufVO::new([1, 1, 0].as_slice().into(), addr),
        Arc::new(Semaphore::new(1)),
        clients.clone(),
        router.get_handler_by_method_id(&1).unwrap(),
        queue.clone(),
        Arc::new(StateRegistry::new()),
        old_handle.clone(),
    )
    .await;

    let (_new_completion, new_observer) = SessionObserver::new();
    let (transport, mut peer) = tokio::io::duplex(64);
    let (new_user, new_writer, new_removed) = LynnUser::new(
        Box::new(transport),
        Arc::new(RwLock::new(SystemTime::now())),
        0,
    );
    let new_handle = SessionHandle::new(&new_user, new_observer, new_removed, tasks, (9177, 7719));
    clients.insert(addr, new_user);
    drop(old_completion);
    old_writer.await.unwrap();

    let event = queue.steal().success().unwrap();
    let EventType::ExecuteTask((route, context, _, permit)) = event.event_type else {
        return Err("expected the queued original request");
    };
    route.run(context).await.unwrap();
    drop(permit);
    let late = observed.recv().await.unwrap();
    assert_eq!(late.id(), old_handle.id());
    assert_ne!(late.id(), new_handle.id());
    assert!(late.observer().is_closed());
    assert!(!new_handle.observer().is_closed());
    assert_eq!(
        late.try_send(2, "private old".into()).unwrap(),
        SessionSendOutcome::Closed
    );
    assert_eq!(
        new_handle.try_send(3, "new".into()).unwrap(),
        SessionSendOutcome::Enqueued
    );
    let mut frame = [0; 18];
    timeout(Duration::from_secs(1), peer.read_exact(&mut frame))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(&frame[11..13], &3_u16.to_le_bytes());
    assert_eq!(&frame[13..16], b"new");
    clients.clear();
    new_writer.await.unwrap();
    Ok(())
}
