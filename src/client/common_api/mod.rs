use std::time::Duration;

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
    task::JoinHandle,
    time::interval,
};
use tracing::{error, info, warn};

use crate::{
    const_config::DEFAULT_MAX_RECEIVE_BYTES_SIZE,
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
    vo_factory::big_buf::BigBufReader,
};

#[inline(always)]
pub(super) fn spawn_handle(
    stream: TcpStream,
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
        let write_handle: JoinHandle<tokio::io::WriteHalf<TcpStream>> = tokio::spawn(async move {
            loop {
                if !rx_write.is_closed() {
                    if let Some(mut handler_result) = rx_write.recv().await {
                        if !handler_result.is_with_mark() {
                            handler_result
                                .set_marks(message_header_mark.clone(), message_tail_mark.clone());
                        }
                        if let Some(response) = handler_result.get_response_data() {
                            if let Err(e) = write_half.write_all(&response).await {
                                error!("write to server failed - e: {:?}", e);
                            }
                        } else {
                            warn!("nothing to send");
                        }
                    }
                } else {
                    break;
                }
            }
            return write_half;
        });
        let mut buf = [0; DEFAULT_MAX_RECEIVE_BYTES_SIZE];
        let mut big_buf = BigBufReader::new(message_header_mark, message_tail_mark);
        loop {
            match read_half.read(&mut buf).await {
                Ok(n) if n <= 0 => {
                    break;
                }
                Ok(n) => {
                    big_buf.extend_from_slice(&buf[..n]);
                    while big_buf.is_complete() {
                        let input_buf_vo = InputBufVO::new_without_socket_addr(big_buf.get_data());
                        if let Err(e) = tx_read.send(input_buf_vo).await {
                            error!("send to channel failed - e: {:?}", e);
                        }
                    }
                }
                Err(e) => {
                    error!("read from server failed : {}", e);
                }
            }
        }
        if let Ok(wirte_half) = write_handle.await {
            if read_half.is_pair_of(&wirte_half) {
                let mut socket = read_half.unsplit(wirte_half);
                socket.shutdown();
            }
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
