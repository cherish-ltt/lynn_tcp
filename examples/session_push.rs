//! Public demonstration registration; applications must authorize private subscriptions.

use std::{collections::HashMap, sync::Arc, time::Duration};

use lynn_tcp::{
    bytes::Bytes,
    lynn_client::LynnClient,
    lynn_middleware::RequestContext,
    lynn_server::{LynnServer, SessionHandle, SessionSendOutcome},
    lynn_tcp_dependents::{AppState, HandlerResult, InputBufVOTrait},
};
use tokio::sync::{Mutex, mpsc};

struct Registrations {
    online: Arc<Mutex<HashMap<&'static str, SessionHandle>>>,
    registered: mpsc::UnboundedSender<SessionHandle>,
}

async fn register(request: RequestContext, state: AppState<Registrations>) -> HandlerResult {
    let session = request.session().clone();
    state.online.lock().await.insert("demo", session.clone());
    state.registered.send(session).unwrap();
    HandlerResult::new_with_send(1, "registered".into(), vec![request.peer_addr()]).unwrap()
}

async fn demonstrate() -> Result<(), Box<dyn std::error::Error>> {
    let (registered, mut registrations) = mpsc::unbounded_channel();
    let online = Arc::new(Mutex::new(HashMap::new()));
    let server = LynnServer::new_with_addr("127.0.0.1:0")
        .await
        .with_state(Registrations {
            online: online.clone(),
            registered,
        })
        .add_router(1, register);
    let ready = server.startup_handle();
    let server_task = tokio::spawn(server.start());
    let addr = ready.wait().await?;
    let mut clients = Vec::new();
    let mut cleanup_tasks = Vec::new();
    for _ in 0..2 {
        let mut client = LynnClient::new_with_addr(addr).await.start().await;
        client
            .send_data(HandlerResult::new_with_send_to_server(1, Bytes::new())?)
            .await?;
        let reply = client
            .get_receive_data()
            .await
            .ok_or("registration connection closed")?;
        assert_eq!(reply.get_all_bytes().as_ref(), b"registered");
        let session = registrations.recv().await.ok_or("registration missing")?;
        let online = online.clone();
        cleanup_tasks.push(tokio::spawn(async move {
            assert_eq!(
                session.try_send(2, "background result".into()).unwrap(),
                SessionSendOutcome::Enqueued
            );
            let observer = session.observer();
            observer.closed().await;
            let mut online = online.lock().await;
            if online
                .get("demo")
                .is_some_and(|current| current.id() == observer.id())
            {
                online.remove("demo");
            }
            assert_eq!(
                session.try_send(2, Bytes::new()).unwrap(),
                SessionSendOutcome::Closed
            );
        }));
        let reply = client
            .get_receive_data()
            .await
            .ok_or("push connection closed")?;
        assert_eq!(reply.get_all_bytes().as_ref(), b"background result");
        clients.push(client);
    }
    drop(clients.remove(0));
    cleanup_tasks.remove(0).await?;
    assert_eq!(online.lock().await.len(), 1);
    drop(clients);
    cleanup_tasks.remove(0).await?;
    assert!(online.lock().await.is_empty());
    server_task.abort();
    assert!(server_task.await.unwrap_err().is_cancelled());
    println!("Verified private push, final close and identity-checked cleanup.");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::time::timeout(Duration::from_secs(10), demonstrate()).await?
}
