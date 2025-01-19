/// The default server address.
pub(crate) const DEFAULT_IPV4: &str = "0.0.0.0:9177";
/// The maximum number of connections allowed by the server.
pub(crate) const DEFAULT_MAX_CONNECTIONS: usize = 1000;
/// The maximum number of threads allowed by the server.
pub(crate) const DEFAULT_MAX_THREADPOOL_SIZE: usize = 10;
/// The default maximum receive data size for a single client in bytes.
pub(crate) const DEFAULT_MAX_RECEIVE_BYTES_SIZE: usize = 1024;
/// The default maximum communication channel size for a single client.
/// To avoid memory overflow in case of network delay, it is set to 3.
pub(crate) const DEFAULT_CHANNEL_SIZE: usize = 3;
/// The default maximum processing permit size for a single client.
/// Assuming the single processing delay is controlled within 100ms, the server allows processing at least 7 requests from a single client within 100ms.
pub(crate) const DEFAULT_PROCESS_PERMIT_SIZE: usize = 7;
/// The interval for heartbeat detection in seconds.
pub(crate) const DEFAULT_CHECK_HEART_INTERVAL: u64 = 5;
/// The timeout time for heartbeat detection in seconds.
pub(crate) const DEFAULT_CHECK_HEART_TIMEOUT_TIME: u64 = 30;
