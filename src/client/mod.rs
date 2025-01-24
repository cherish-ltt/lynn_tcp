mod lynn_client_config;

use std::time::Duration;

use lynn_client_config::{LynnClientConfig, LynnClientConfigBuilder};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    sync::mpsc,
    task::JoinHandle,
    time::{self, interval},
};
use tracing::{error, info, warn, Level};
use tracing_subscriber::fmt;

use crate::{
    const_config::DEFAULT_MAX_RECEIVE_BYTES_SIZE,
    lynn_tcp_dependents::{HandlerResult, InputBufVO},
    vo_factory::big_buf::BigBufReader,
};

pub mod client_config{
    pub use super::lynn_client_config::LynnClientConfigBuilder;
    pub use super::lynn_client_config::LynnClientConfig;
}

#[cfg(feature = "client")]
pub struct LynnClient<'a> {
    lynn_client_config: LynnClientConfig<'a>,
    connection_join_handle: Option<JoinHandle<()>>,
    tx_write: Option<mpsc::Sender<HandlerResult>>,
    rx_read: Option<mpsc::Receiver<InputBufVO>>,
}

impl<'a> LynnClient<'a> {
    pub async fn new_with_config(lynn_client_config: LynnClientConfig<'a>) -> Self {
        let client = Self {
            lynn_client_config,
            connection_join_handle: None,
            tx_write: None,
            rx_read: None,
        };
        client.log_server().await;
        client
    }

    pub async fn new_with_ipv4(server_ipv4: &'a str) -> Self {
        let client = Self {
            lynn_client_config: LynnClientConfigBuilder::new()
                .with_server_ipv4(server_ipv4)
                .build(),
            connection_join_handle: None,
            tx_write: None,
            rx_read: None,
        };
        client.log_server().await;
        client
    }

    pub async fn start(mut self: Self) -> Self {
        match self.run().await {
            Ok(_) => self,
            Err(e) => {
                error!("{}", e);
                self
            }
        }
    }

    async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let retry_count = 3;
        let timeout = Duration::from_secs(3);
        let ip_v4 = self
            .lynn_client_config
            .get_server_ipv4()
            .clone()
            .to_string();
        let channel_size = self
            .lynn_client_config
            .get_client_single_channel_size()
            .clone();
        let message_header_mark = self.lynn_client_config.get_message_header_mark().clone();
        let message_tail_mark = self.lynn_client_config.get_message_tail_mark().clone();
        for _ in 0..retry_count {
            match time::timeout(timeout, TcpStream::connect(ip_v4.clone())).await {
                Ok(stream) => {
                    if let Ok(stream) = stream {
                        let (tx_read, rx_read) = mpsc::channel::<InputBufVO>(channel_size);
                        let (tx_write, mut rx_write) = mpsc::channel::<HandlerResult>(channel_size);
                        let join_handle = tokio::spawn(async move {
                            let (mut read_half, mut write_half) = tokio::io::split(stream);
                            let message_header_mark_clone = message_header_mark.clone();
                            let message_tail_mark_clone = message_tail_mark.clone();
                            tokio::spawn(async move {
                                while let Some(mut handler_result) = rx_write.recv().await {
                                    if !handler_result.is_with_mark() {
                                        handler_result.set_marks(
                                            message_header_mark_clone.clone(),
                                            message_tail_mark_clone.clone(),
                                        );
                                    }
                                    if let Some(response) = handler_result.get_response_data() {
                                        if let Err(e) = write_half.write_all(&response).await {
                                            error!("write to server failed - e: {:?}", e);
                                        }
                                    } else {
                                        warn!("nothing to send");
                                    }
                                }
                            });
                            let mut buf = [0; DEFAULT_MAX_RECEIVE_BYTES_SIZE];
                            let mut big_buf =
                                BigBufReader::new(message_header_mark, message_tail_mark);
                            loop {
                                match read_half.read(&mut buf).await {
                                    Ok(n) if n <= 0 => {
                                        continue;
                                    }
                                    Ok(n) => {
                                        big_buf.extend_from_slice(&buf[..n]);
                                        while big_buf.is_complete() {
                                            let input_buf_vo = InputBufVO::new_without_socket_addr(
                                                big_buf.get_data(),
                                            );
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
                        });
                        self.tx_write = Some(tx_write);
                        self.rx_read = Some(rx_read);
                        self.connection_join_handle = Some(join_handle);
                        //self.check_heart().await;
                        info!("Client - [Main-LynnClient] connection to [server_ipv4:{}] success!!! ",{ip_v4});
                        return Ok(());
                    } else {
                        warn!("connect to server failed - TcpStream e: {:?}", stream);
                        continue;
                    }
                }
                Err(e) => {
                    warn!("connect to server failed - timeout e: {:?}", e);
                    continue;
                }
            }
        }
        Err("connect to server failed".into())
    }

    pub(crate) async fn log_server(&self) {
        let subscriber = fmt::Subscriber::builder()
            .with_max_level(Level::DEBUG)
            .finish();
        match tracing::subscriber::set_global_default(subscriber) {
            Ok(_) => {
                info!("Client - [log server] start sucess!!!")
            },
            Err(e) => {
                warn!("set_global_default failed - e: {:?}", e)
            },
        }
    }

    pub async fn get_receive_data(&mut self) -> Option<InputBufVO> {
        self.rx_read.as_mut().unwrap().recv().await
    }

    pub async fn get_sender(&mut self) -> Option<mpsc::Sender<HandlerResult>> {
        self.tx_write.clone()
    }

    pub async fn send_data(
        &mut self,
        handler_result: HandlerResult,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match &self.tx_write {
            Some(sender) => {
                if let Err(e) = sender.send(handler_result).await {
                    error!("send to server failed - e: {:?}", e);
                    Err(e.into())
                } else {
                    Ok(())
                }
            }
            None => Err("tx_write is None , No linked server".into()),
        }
    }

    pub(crate) async fn check_heart(&mut self) {
        let interval_time = self
            .lynn_client_config
            .get_server_check_heart_interval()
            .clone();
        if let Some(sender) = self.get_sender().await {
            tokio::spawn(async move {
                info!(
                    "Client - [check heart] start sucess!!! with [client_check_heart_interval:{}s]",
                    interval_time
                );
                let mut interval = interval(Duration::from_secs(interval_time));
                loop {
                    interval.tick().await;
                    if let Err(e) = sender
                        .send(HandlerResult::new_with_send_to_server(0_u16, vec![]))
                        .await
                    {
                        error!("send to server failed - e: {:?}", e)
                    }
                }
            });
        } else {
            warn!("Client - [check heart] start failed!!!");
        }
    }
}
