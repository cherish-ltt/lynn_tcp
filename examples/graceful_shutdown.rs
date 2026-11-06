use std::{sync::Arc, time::Duration};

use lynn_tcp::{
    lynn_server::{LynnServer, LynnServerConfigBuilder, ShutdownOutcome},
    lynn_state::AppState,
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{Notify, oneshot},
    time::timeout,
};

struct Work {
    accepted: Notify,
    finish: Notify,
}

async fn delayed(input: InputBufVO, work: AppState<Arc<Work>>) -> HandlerResult {
    work.accepted.notify_one();
    work.finish.notified().await;
    HandlerResult::new_with_send(1, "finished".into(), vec![input.get_input_addr().unwrap()])
        .expect("fixed payload fits the protocol")
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    timeout(Duration::from_secs(5), demonstrate()).await??;
    Ok(())
}

async fn demonstrate() -> Result<(), Box<dyn std::error::Error>> {
    let work = Arc::new(Work {
        accepted: Notify::new(),
        finish: Notify::new(),
    });
    let config = LynnServerConfigBuilder::new()
        .with_addr("127.0.0.1:0")?
        .with_server_max_taskpool_size(&2)
        .build();
    let server = LynnServer::new_with_config(config)
        .await
        .with_state(work.clone())
        .add_router(1, delayed);
    let ready = server.startup_handle();
    let (stop, shutdown) = oneshot::channel();
    let task = tokio::spawn(server.start_with_shutdown(
        async {
            let _ = shutdown.await;
        },
        Duration::from_secs(2),
    ));
    let addr = ready.wait().await?;
    let mut peer = TcpStream::connect(addr).await?;
    let mut request = 9177u16.to_le_bytes().to_vec();
    request.extend_from_slice(&5u64.to_le_bytes());
    request.extend_from_slice(&[1, 1, 0]);
    request.extend_from_slice(&7719u16.to_le_bytes());
    peer.write_all(&request).await?;
    work.accepted.notified().await;
    stop.send(()).expect("server is running");
    // The application supplies the signal; synchronizing here demonstrates the drain phase.
    while TcpStream::connect(addr).await.is_ok() {
        tokio::task::yield_now().await;
    }
    work.finish.notify_one();
    let mut response = Vec::new();
    peer.read_to_end(&mut response).await?;
    assert_eq!(&response[13..response.len() - 2], b"finished");
    let outcome = task.await??;
    assert_eq!(outcome, ShutdownOutcome::Drained);
    println!("{outcome:?}: received the accepted request's full response before EOF");
    println!(
        "Drained is transport completion; business delivery needs an application acknowledgement."
    );
    Ok(())
}
