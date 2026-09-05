use std::{sync::Arc, time::Duration};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::{
        Mutex,
        mpsc::{self, Sender},
        watch,
    },
    task::JoinHandle,
    time::interval,
};
use tracing::{error, info, warn};

use crate::domain::model::handler_result::HandlerResult;
use crate::domain::model::input_buf_vo::InputBufVO;
use crate::infrastructure::protocol::big_buf_reader::BigBufReader;
use crate::infrastructure::tcp::stream::LynnStream;
use crate::{LynnError, const_config::DEFAULT_MAX_RECEIVE_BYTES_SIZE};

/// The shared write-side receiver, owned by the connection supervisor and
/// borrowed by each connection's write pump, so the user-facing
/// `mpsc::Sender<HandlerResult>` stays valid across reconnections.
pub(super) type SharedWriteReceiver = Arc<Mutex<mpsc::Receiver<HandlerResult>>>;

/// Spawns the read and write pumps for one connection.
///
/// Returns the read pump's handle: it resolves as soon as the connection is
/// considered dead (read EOF/error or the user-side receiver was dropped).
/// The write pump exits independently once `close_rx` fires or the write
/// channel closes, and shuts its half down on the way out.
#[inline(always)]
pub(super) fn spawn_connection_pumps(
    stream: LynnStream,
    message_header_mark: u16,
    message_tail_mark: u16,
    tx_read: Sender<InputBufVO>,
    rx_write: SharedWriteReceiver,
    close_rx: watch::Receiver<bool>,
) -> JoinHandle<()> {
    let (read_half, write_half) = tokio::io::split(stream);
    let write_handle = tokio::spawn(write_pump(
        write_half,
        message_header_mark,
        message_tail_mark,
        rx_write,
        close_rx,
    ));
    let read_handle = tokio::spawn(read_pump(
        read_half,
        message_header_mark,
        message_tail_mark,
        tx_read,
    ));
    let _ = write_handle; // exits via close_rx / channel closure
    read_handle
}

/// Reads frames from the connection and forwards them to the user channel.
async fn read_pump(
    mut read_half: tokio::io::ReadHalf<LynnStream>,
    message_header_mark: u16,
    message_tail_mark: u16,
    tx_read: Sender<InputBufVO>,
) {
    let mut buf = [0; DEFAULT_MAX_RECEIVE_BYTES_SIZE];
    let mut big_buf = BigBufReader::new(message_header_mark, message_tail_mark);
    'read_loop: loop {
        match read_half.read(&mut buf).await {
            Ok(0) => {
                break;
            },
            Ok(n) => {
                big_buf.extend_from_slice(&buf[..n]);
                while big_buf.is_complete() {
                    let input_buf_vo = InputBufVO::new_without_socket_addr(big_buf.get_data());
                    if tx_read.send(input_buf_vo).await.is_err() {
                        // Receiver dropped: stop feeding the connection.
                        break 'read_loop;
                    }
                }
            },
            Err(e) => {
                // A persistent read error (e.g. a torn-down TLS session
                // errors on every poll) must terminate the loop, otherwise
                // this task spins forever.
                error!("read from server failed : {}", e);
                break;
            },
        }
    }
}

/// Writes queued frames to the connection until it is closed or told to stop.
async fn write_pump(
    mut write_half: tokio::io::WriteHalf<LynnStream>,
    message_header_mark: u16,
    message_tail_mark: u16,
    rx_write: SharedWriteReceiver,
    mut close_rx: watch::Receiver<bool>,
) {
    loop {
        let received = tokio::select! {
            changed = close_rx.changed() => {
                if changed.is_err() || *close_rx.borrow() {
                    let _ = write_half.shutdown().await;
                    break;
                }
                continue;
            },
            received = async { rx_write.lock().await.recv().await } => received,
        };
        match received {
            Some(mut handler_result) => {
                if !handler_result.is_with_mark() {
                    handler_result.set_marks(message_header_mark, message_tail_mark);
                }
                if let Some(response) = handler_result.get_response_data() {
                    if let Err(e) = write_half.write_all(&response).await {
                        // Connection is gone: stop writing, otherwise a
                        // persistent transport error would spin this task.
                        error!("write to server failed - e: {:?}", e);
                        break;
                    }
                } else {
                    warn!("nothing to send");
                }
            },
            None => {
                // All user-side senders dropped: client is gone.
                let _ = write_half.shutdown().await;
                break;
            },
        }
    }
}

#[inline(always)]
pub(super) fn spawn_check_heart(interval_time: u64, sender: mpsc::Sender<HandlerResult>) {
    tokio::spawn(async move {
        info!(
            "Client - [check heart] start sucess!!! with [client_check_heart_interval:{}s]",
            interval_time
        );
        let mut interval = interval(Duration::from_secs(interval_time));
        loop {
            interval.tick().await;
            if !sender.is_closed() {
                if let Err(e) = sender
                    .send(HandlerResult::new_with_send_heart_to_server())
                    .await
                {
                    error!("send to server failed - e: {:?}", e)
                }
            } else {
                break;
            }
        }
    });
}
