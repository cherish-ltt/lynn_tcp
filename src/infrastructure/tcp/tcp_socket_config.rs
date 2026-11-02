/// TCP socket configuration.
pub(crate) struct TcpSocketConfig {
    pub nodelay: bool,
    pub keepalive_enabled: bool,
    pub keepalive_time_secs: u64,
    pub recv_buffer_size: usize,
    pub send_buffer_size: usize,
    pub read_timeout_secs: u64,
    pub write_timeout_secs: u64,
}
