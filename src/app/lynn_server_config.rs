use crate::const_config::{
    DEFAULT_CHANNEL_SIZE, DEFAULT_CHECK_HEART_INTERVAL, DEFAULT_CHECK_HEART_TIMEOUT_TIME,
    DEFAULT_IPV4, DEFAULT_MAX_CONNECTIONS, DEFAULT_MAX_RECEIVE_BYTES_SIZE,
    DEFAULT_MAX_THREADPOOL_SIZE, DEFAULT_MESSAGE_HEADER_MARK, DEFAULT_MESSAGE_TAIL_MARK,
    DEFAULT_PROCESS_PERMIT_SIZE,
};

/// Represents the configuration for the Lynn server.
///
/// This struct contains various configuration parameters for the server,
/// such as the IP address, channel size, maximum connections, thread pool size, etc.
#[cfg(feature = "server")]
pub struct LynnServerConfig<'a> {
    // The IPv4 address of the server.
    server_ipv4: &'a str,
    // The size of a single channel.
    server_single_channel_size: &'a usize,
    // The maximum number of connections for the server.
    server_max_connections: Option<&'a usize>,
    // The maximum number of threads for the server.
    server_max_threadpool_size: &'a usize,
    // The maximum number of bytes the server can receive.
    server_max_receive_bytes_reader_size: &'a usize,
    // The permit size for a single process.
    server_single_processs_permit: &'a usize,
    // The interval for checking heartbeats.
    server_check_heart_interval: &'a u64,
    // The timeout time for checking heartbeats.
    server_check_heart_timeout_time: &'a u64,
    message_header_mark: &'a u16,
    message_tail_mark: &'a u16,
}

/// Implementation for LynnServerConfig, providing methods to create and get the configuration.
///
/// This implementation includes a constructor for the default configuration and methods to get the configuration parameters.
impl<'a> LynnServerConfig<'a> {
    /// Creates a new LynnServerConfig instance with the given parameters.
    ///
    /// # Parameters
    ///
    /// * `server_ipv4` - The IPv4 address of the server.
    /// * `server_single_channel_size` - The size of a single channel.
    /// * `server_max_connections` - The maximum number of connections for the server.
    /// * `server_max_threadpool_size` - The maximum number of threads for the server.
    /// * `server_max_receive_bytes_reader_size` - The maximum number of bytes the server can receive.
    /// * `server_single_processs_permit` - The permit size for a single process.
    /// * `server_check_heart_interval` - The interval for checking heartbeats.
    /// * `server_check_heart_timeout_time` - The timeout time for checking heartbeats.
    ///
    /// # Returns
    ///
    /// A new LynnServerConfig instance.
    fn new(
        server_ipv4: &'a str,
        server_single_channel_size: &'a usize,
        server_max_connections: Option<&'a usize>,
        server_max_threadpool_size: &'a usize,
        server_max_receive_bytes_reader_size: &'a usize,
        server_single_processs_permit: &'a usize,
        server_check_heart_interval: &'a u64,
        server_check_heart_timeout_time: &'a u64,
        message_header_mark: &'a u16,
        message_tail_mark: &'a u16,
    ) -> Self {
        Self {
            server_ipv4,
            server_max_connections,
            server_max_threadpool_size,
            server_max_receive_bytes_reader_size,
            server_single_channel_size,
            server_single_processs_permit,
            server_check_heart_interval,
            server_check_heart_timeout_time,
            message_header_mark,
            message_tail_mark,
        }
    }

    /// Creates a default LynnServerConfig instance.
    ///
    /// # Returns
    ///
    /// A default LynnServerConfig instance.
    pub(crate) fn default() -> Self {
        Self {
            server_ipv4: DEFAULT_IPV4,
            server_max_connections: Some(&DEFAULT_MAX_CONNECTIONS),
            server_max_threadpool_size: &DEFAULT_MAX_THREADPOOL_SIZE,
            server_max_receive_bytes_reader_size: &DEFAULT_MAX_RECEIVE_BYTES_SIZE,
            server_single_channel_size: &DEFAULT_CHANNEL_SIZE,
            server_single_processs_permit: &DEFAULT_PROCESS_PERMIT_SIZE,
            server_check_heart_interval: &DEFAULT_CHECK_HEART_INTERVAL,
            server_check_heart_timeout_time: &DEFAULT_CHECK_HEART_TIMEOUT_TIME,
            message_header_mark: &DEFAULT_MESSAGE_HEADER_MARK,
            message_tail_mark: &DEFAULT_MESSAGE_TAIL_MARK,
        }
    }

    /// Gets the IPv4 address of the server.
    ///
    /// # Returns
    ///
    /// The IPv4 address of the server.
    pub(crate) fn get_server_ipv4(&self) -> &str {
        &self.server_ipv4
    }

    /// Gets the size of a single channel.
    ///
    /// # Returns
    ///
    /// The size of a single channel.
    pub(crate) fn get_server_single_channel_size(&self) -> &usize {
        &self.server_single_channel_size
    }

    /// Gets the permit size for a single process.
    ///
    /// # Returns
    ///
    /// The permit size for a single process.
    pub(crate) fn get_server_single_processs_permit(&self) -> &usize {
        self.server_single_processs_permit
    }

    /// Gets the interval for checking heartbeats.
    ///
    /// # Returns
    ///
    /// The interval for checking heartbeats.
    pub(crate) fn get_server_check_heart_interval(&self) -> &u64 {
        self.server_check_heart_interval
    }

    /// Gets the timeout time for checking heartbeats.
    ///
    /// # Returns
    ///
    /// The timeout time for checking heartbeats.
    pub(crate) fn get_server_check_heart_timeout_time(&self) -> &u64 {
        self.server_check_heart_timeout_time
    }

    /// Gets the maximum number of connections for the server.
    ///
    /// # Returns
    ///
    /// The maximum number of connections for the server.
    pub(crate) fn get_server_max_connections(&self) -> Option<&usize> {
        self.server_max_connections
    }

    /// Gets the maximum number of threads for the server.
    ///
    /// # Returns
    ///
    /// The maximum number of threads for the server.
    pub(crate) fn get_server_max_threadpool_size(&self) -> &usize {
        self.server_max_threadpool_size
    }

    /// Gets the maximum number of bytes the server can receive.
    ///
    /// # Returns
    ///
    /// The maximum number of bytes the server can receive.
    pub(crate) fn get_server_max_receive_bytes_reader_size(&self) -> &usize {
        self.server_max_receive_bytes_reader_size
    }

    pub(crate) fn get_message_header_mark(&self) -> &u16 {
        self.message_header_mark
    }

    pub(crate) fn get_message_tail_mark(&self) -> &u16 {
        self.message_tail_mark
    }
}

/// Builder for constructing a LynnServerConfig instance.
///
/// This builder provides a series of methods to set the various parameters of LynnServerConfig
/// and finally builds a LynnServerConfig instance.
#[cfg(feature = "server")]
pub struct LynnServerConfigBuilder<'a> {
    pub lynn_config: LynnServerConfig<'a>,
}

/// Implementation for LynnServerConfigBuilder, providing methods to set the configuration parameters.
///
/// This implementation includes a constructor for the default configuration and methods to set the configuration parameters.
impl<'a> LynnServerConfigBuilder<'a> {
    /// Creates a new LynnServerConfigBuilder instance.
    ///
    /// # Returns
    ///
    /// A new LynnServerConfigBuilder instance.
    pub fn new() -> Self {
        Self {
            lynn_config: LynnServerConfig::default(),
        }
    }

    /// Sets the IPv4 address of the server.
    ///
    /// # Parameters
    ///
    /// * `server_ipv4` - The IPv4 address of the server.
    ///
    /// # Returns
    ///
    /// The modified LynnServerConfigBuilder instance.
    pub fn with_server_ipv4(mut self, server_ipv4: &'a str) -> Self {
        self.lynn_config.server_ipv4 = server_ipv4;
        self
    }

    /// Sets the size of a single channel.
    ///
    /// # Parameters
    ///
    /// * `server_single_channel_size` - The size of a single channel.
    ///
    /// # Returns
    ///
    /// The modified LynnServerConfigBuilder instance.
    pub fn with_server_single_channel_size(
        mut self,
        server_single_channel_size: &'a usize,
    ) -> Self {
        self.lynn_config.server_single_channel_size = server_single_channel_size;
        self
    }

    /// Sets the permit size for a single process.
    ///
    /// # Parameters
    ///
    /// * `server_single_processs_permit` - The permit size for a single process.
    ///
    /// # Returns
    ///
    /// The modified LynnServerConfigBuilder instance.
    pub fn with_server_single_processs_permit(
        mut self,
        server_single_processs_permit: &'a usize,
    ) -> Self {
        self.lynn_config.server_single_processs_permit = server_single_processs_permit;
        self
    }
    /// Sets the interval for checking heartbeats.
    ///
    /// # Parameters
    ///
    /// * `server_check_heart_interval` - The interval for checking heartbeats.
    ///
    /// # Returns
    ///
    /// The modified LynnServerConfigBuilder instance.
    pub fn with_server_check_heart_interval(
        mut self,
        server_check_heart_interval: &'a u64,
    ) -> Self {
        self.lynn_config.server_check_heart_interval = server_check_heart_interval;
        self
    }

    /// Sets the timeout time for checking heartbeats.
    ///
    /// # Parameters
    ///
    /// * `server_check_heart_timeout_time` - The timeout time for checking heartbeats.
    ///
    /// # Returns
    ///
    /// The modified LynnServerConfigBuilder instance.
    pub fn with_server_check_heart_timeout_time(
        mut self,
        server_check_heart_timeout_time: &'a u64,
    ) -> Self {
        self.lynn_config.server_check_heart_timeout_time = server_check_heart_timeout_time;
        self
    }

    /// Sets the maximum number of connections for the server.
    ///
    /// # Parameters
    ///
    /// * `server_max_connections` - The maximum number of connections for the server.
    ///
    /// # Returns
    ///
    /// The modified LynnServerConfigBuilder instance.
    pub fn with_server_max_connections(
        mut self,
        server_max_connections: Option<&'a usize>,
    ) -> Self {
        self.lynn_config.server_max_connections = server_max_connections;
        self
    }

    /// Sets the maximum number of threads for the server.
    ///
    /// # Parameters
    ///
    /// * `server_max_threadpool_size` - The maximum number of threads for the server.
    ///
    /// # Returns
    ///
    /// The modified LynnServerConfigBuilder instance.
    pub fn with_server_max_threadpool_size(
        mut self,
        server_max_threadpool_size: &'a usize,
    ) -> Self {
        self.lynn_config.server_max_threadpool_size = server_max_threadpool_size;
        self
    }

    /// Sets the maximum number of bytes the server can receive.
    ///
    /// # Parameters
    ///
    /// * `server_max_receive_bytes_reader_size` - The maximum number of bytes the server can receive.
    ///
    /// # Returns
    ///
    /// The modified LynnServerConfigBuilder instance.
    pub fn with_server_max_receive_bytes_reader_size(
        mut self,
        server_max_receive_bytes_reader_size: &'a usize,
    ) -> Self {
        self.lynn_config.server_max_receive_bytes_reader_size =
            server_max_receive_bytes_reader_size;
        self
    }

    /// Builds and returns the final LynnServerConfig instance.
    ///
    /// # Returns
    ///
    /// The final LynnServerConfig instance.
    pub fn build(self) -> LynnServerConfig<'a> {
        self.lynn_config
    }

    pub(crate) fn with_message_header_mark(mut self, msg_header_mark: &'a u16) -> Self {
        self.lynn_config.message_header_mark = msg_header_mark;
        self
    }

    pub(crate) fn with_message_tail_mark(mut self, msg_tail_mark: &'a u16) -> Self {
        self.lynn_config.message_tail_mark = msg_tail_mark;
        self
    }
}
