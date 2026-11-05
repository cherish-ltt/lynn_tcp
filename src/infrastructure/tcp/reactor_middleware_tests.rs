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
                RequestContext::new(1, addr),
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
