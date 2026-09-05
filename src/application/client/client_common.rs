use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
    task::JoinHandle,
    time::interval,
};
use tracing::{error, info, warn};

use crate::domain::model::handler_result::HandlerResult;
use crate::domain::model::input_buf_vo::InputBufVO;
use crate::infrastructure::protocol::big_buf_reader::BigBufReader;
use crate::infrastructure::tcp::stream::LynnStream;
use crate::{LynnError, const_config::DEFAULT_MAX_RECEIVE_BYTES_SIZE};

/// Parameters for a single connection attempt.
pub(super) struct ConnectParams<'a> {
    /// The server address ("ip:port").
    pub(super) addr: &'a str,
    #[cfg(feature = "tls")]
    /// Optional TLS endpoint (connector + resolved server name).
    pub(super) tls: Option<&'a crate::infrastructure::tls::tls_provider::ClientTls>,
}

/// Performs one connection attempt, wrapping the TCP stream into a TLS 1.3
/// session when a TLS endpoint is configured.
pub(super) async fn connect_stream(params: ConnectParams<'_>) -> Result<LynnStream, LynnError> {
    let tcp = TcpStream::connect(params.addr).await?;
    #[cfg(feature = "tls")]
    if let Some(tls) = params.tls {
        let tls_stream = tls
            .connector
            .connect(tls.server_name.clone(), tcp)
            .await
            .map_err(|e| {
                LynnError::tls(format!("TLS handshake with {} failed: {e}", params.addr))
            })?;
        return Ok(LynnStream::Tls(tls_stream.into()));
    }
    Ok(LynnStream::Plain(tcp))
}

#[inline(always)]
pub(super) fn spawn_handle(
    stream: LynnStream,
    channel_size: usize,
    message_header_mark: u16,
    message_tail_mark: u16,
) -> (
    mpsc::Sender<HandlerResult>,
    mpsc::Receiver<InputBufVO>,
    JoinHandle<()>,
) {
    let (tx_read, rx_read) = mpsc::channel::<InputBufVO>(channel_size);
    let (tx_write, mut rx_write) = mpsc::channel::<HandlerResult>(channel_size);
    let join_handle = tokio::spawn(async move {
        let (mut read_half, mut write_half) = tokio::io::split(stream);
        let write_handle: JoinHandle<tokio::io::WriteHalf<LynnStream>> = tokio::spawn(async move {
            loop {
                if !rx_write.is_closed() {
                    if let Some(mut handler_result) = rx_write.recv().await {
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
                    }
                } else {
                    break;
                }
            }
            write_half
        });
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
        if let Ok(wirte_half) = write_handle.await
            && read_half.is_pair_of(&wirte_half)
        {
            let mut socket = read_half.unsplit(wirte_half);
            let _ = socket.shutdown();
        }
    });
    (tx_write, rx_read, join_handle)
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
