use std::{
    net::SocketAddr,
    sync::{LazyLock, OnceLock},
};

pub(crate) const DEFAULT_ADDR: LazyLock<SocketAddr> = LazyLock::new(|| {
    let addr = DEFAULT_IPV4.parse::<SocketAddr>().unwrap();
    addr
});
/// The default server address.
pub(crate) const DEFAULT_IPV4: &str = "0.0.0.0:9177";
/// The maximum number of connections allowed by the server.
pub(crate) const DEFAULT_MAX_CONNECTIONS: usize = 1000;
/// The maximum number of threads allowed by the server.
pub(crate) const DEFAULT_MAX_THREADPOOL_SIZE: usize = 10;
/// The default maximum receive data size for a single client in bytes.
pub(crate) const DEFAULT_MAX_RECEIVE_BYTES_SIZE: usize = 1024 * 8;
/// The default system channel size, used to store the maximum number of messages that can be buffered in the system channel.
pub(crate) const DEFAULT_SYSTEM_CHANNEL_SIZE: usize = 64;
/// The default maximum processing permit size for a single client.
/// Assuming the single processing delay is controlled within 100ms, the server allows processing at least 14 requests from a single client within 100ms.
pub(crate) const DEFAULT_PROCESS_PERMIT_SIZE: usize = 14;
/// The interval for heartbeat detection in seconds.
pub(crate) const DEFAULT_CHECK_HEART_INTERVAL: u64 = 5;
/// The timeout time for heartbeat detection in seconds.
pub(crate) const DEFAULT_CHECK_HEART_TIMEOUT_TIME: u64 = 30;
/// The default message header mark, used to identify the start of a message.
pub(crate) const DEFAULT_MESSAGE_HEADER_MARK: u16 = 9177;
/// The default message tail mark, used to identify the end of a message.
pub(crate) const DEFAULT_MESSAGE_TAIL_MARK: u16 = 7719;
/// A OnceLock for the server message header mark, used to store the message header mark for the server.
pub(crate) static SERVER_MESSAGE_HEADER_MARK: OnceLock<u16> = OnceLock::new();
/// A OnceLock for the server message tail mark, used to store the message tail mark for the server.
pub(crate) static SERVER_MESSAGE_TAIL_MARK: OnceLock<u16> = OnceLock::new();
