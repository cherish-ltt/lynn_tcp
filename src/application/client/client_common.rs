use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc::{self, Sender},
    task::JoinHandle,
    time::interval,
};
use tracing::{error, info, warn};

use crate::const_config::DEFAULT_MAX_RECEIVE_BYTES_SIZE;
use crate::domain::model::handler_result::HandlerResult;
use crate::domain::model::input_buf_vo::InputBufVO;
use crate::infrastructure::protocol::big_buf_reader::BigBufReader;
use crate::infrastructure::tcp::stream::LynnStream;

/// Runs the read and write pumps for one connection.
///
/// Both pumps belong to the supervisor future. Ending either pump or
/// cancelling the supervisor drops both halves, including blocked I/O.
#[inline(always)]
pub(super) async fn run_connection_pumps(
    stream: LynnStream,
    message_header_mark: u16,
    message_tail_mark: u16,
    tx_read: Sender<InputBufVO>,
    rx_write: &mut mpsc::Receiver<HandlerResult>,
) {
    let (read_half, write_half) = tokio::io::split(stream);
    tokio::select! {
        () = write_pump(write_half, message_header_mark, message_tail_mark, rx_write) => {},
        () = read_pump(read_half, message_header_mark, message_tail_mark, tx_read) => {},
    }
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

/// Writes queued frames until the channel closes or a transport error occurs.
async fn write_pump(
    mut write_half: tokio::io::WriteHalf<LynnStream>,
    message_header_mark: u16,
    message_tail_mark: u16,
    rx_write: &mut mpsc::Receiver<HandlerResult>,
) {
    loop {
        let received = rx_write.recv().await;
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
pub(super) fn spawn_check_heart(
    interval_time: u64,
    sender: mpsc::Sender<HandlerResult>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        info!(
            "Client - [check heart] start success!!! with [client_check_heart_interval:{}s]",
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
    })
}
