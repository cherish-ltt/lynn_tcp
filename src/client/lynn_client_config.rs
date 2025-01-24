use crate::const_config::{
    DEFAULT_CHECK_HEART_INTERVAL, DEFAULT_IPV4, DEFAULT_MESSAGE_HEADER_MARK,
    DEFAULT_MESSAGE_TAIL_MARK, DEFAULT_SYSTEM_CHANNEL_SIZE,
};

/// The configuration for the Lynn client.
///
/// This struct holds the configuration options for the Lynn client, including the server's IPv4 address,
/// the size of the client's single channel, the interval for checking the server's heartbeat, and the marks
/// for the message header and tail.
#[cfg(feature = "client")]
pub struct LynnClientConfig<'a> {
    /// The IPv4 address of the server.
    server_ipv4: &'a str,
    /// The size of the client's single channel.
    client_single_channel_size: &'a usize,
    /// The interval for checking the server's heartbeat.
    client_check_heart_interval: &'a u64,
    /// The mark for the message header.
    message_header_mark: &'a u16,
    /// The mark for the message tail.
    message_tail_mark: &'a u16,
}

impl<'a> LynnClientConfig<'a> {
    /// Creates a new `LynnClientConfig` instance.
    ///
    /// # Parameters
    ///
    /// - `server_ipv4`: The IPv4 address of the server.
    /// - `client_single_channel_size`: The size of the client's single channel.
    /// - `client_check_heart_interval`: The interval for checking the server's heartbeat.
    /// - `message_header_mark`: The mark for the message header.
    /// - `message_tail_mark`: The mark for the message tail.
    ///
    /// # Returns
    ///
    /// A new `LynnClientConfig` instance.
    fn new(
        server_ipv4: &'a str,
        client_single_channel_size: &'a usize,
        client_check_heart_interval: &'a u64,
        message_header_mark: &'a u16,
        message_tail_mark: &'a u16,
    ) -> Self {
        Self {
            server_ipv4,
            client_single_channel_size,
            client_check_heart_interval,
            message_header_mark,
            message_tail_mark,
        }
    }

    /// Returns the default configuration for the Lynn client.
    ///
    /// # Returns
    ///
    /// The default `LynnClientConfig` instance.
    pub(crate) fn default() -> Self {
        Self {
            server_ipv4: DEFAULT_IPV4,
            client_single_channel_size: &DEFAULT_SYSTEM_CHANNEL_SIZE,
            client_check_heart_interval: &DEFAULT_CHECK_HEART_INTERVAL,
            message_header_mark: &DEFAULT_MESSAGE_HEADER_MARK,
            message_tail_mark: &DEFAULT_MESSAGE_TAIL_MARK,
        }
    }

    /// Returns the server's IPv4 address.
    ///
    /// # Returns
    ///
    /// The server's IPv4 address.
    pub(crate) fn get_server_ipv4(&self) -> &str {
        &self.server_ipv4
    }

    /// Sets the server's IPv4 address.
    ///
    /// # Parameters
    ///
    /// - `server_ipv4`: The new IPv4 address of the server.
    pub(crate) fn set_server_ipv4(&mut self, server_ipv4: &'a str) {
        self.server_ipv4 = server_ipv4
    }

    /// Returns the size of the client's single channel.
    ///
    /// # Returns
    ///
    /// The size of the client's single channel.
    pub(crate) fn get_client_single_channel_size(&self) -> &usize {
        &self.client_single_channel_size
    }

    /// Returns the interval for checking the server's heartbeat.
    ///
    /// # Returns
    ///
    /// The interval for checking the server's heartbeat.
    pub(crate) fn get_server_check_heart_interval(&self) -> &u64 {
        self.client_check_heart_interval
    }

    /// Returns the mark for the message header.
    ///
    /// # Returns
    ///
    /// The mark for the message header.
    pub(crate) fn get_message_header_mark(&self) -> &u16 {
        self.message_header_mark
    }

    /// Returns the mark for the message tail.
    ///
    /// # Returns
    ///
    /// The mark for the message tail.
    pub(crate) fn get_message_tail_mark(&self) -> &u16 {
        self.message_tail_mark
    }
}

/// A builder for creating `LynnClientConfig` instances.
///
/// This struct provides a fluent interface for setting the configuration options for the Lynn client.
#[cfg(feature = "server")]
pub struct LynnClientConfigBuilder<'a> {
    /// The configuration for the Lynn client.
    pub lynn_config: LynnClientConfig<'a>,
}

impl<'a> LynnClientConfigBuilder<'a> {
    /// Creates a new `LynnClientConfigBuilder` instance.
    ///
    /// # Returns
    ///
    /// A new `LynnClientConfigBuilder` instance.
    pub fn new() -> Self {
        Self {
            lynn_config: LynnClientConfig::default(),
        }
    }

    /// Sets the server's IPv4 address.
    ///
    /// # Parameters
    ///
    /// - `server_ipv4`: The IPv4 address of the server.
    ///
    /// # Returns
    ///
    /// The `LynnClientConfigBuilder` instance.
    pub fn with_server_ipv4(mut self, server_ipv4: &'a str) -> Self {
        self.lynn_config.server_ipv4 = server_ipv4;
        self
    }

    /// Sets the size of the client's single channel.
    ///
    /// # Parameters
    ///
    /// - `server_single_channel_size`: The size of the client's single channel.
    ///
    /// # Returns
    ///
    /// The `LynnClientConfigBuilder` instance.
    pub fn with_server_single_channel_size(
        mut self,
        server_single_channel_size: &'a usize,
    ) -> Self {
        self.lynn_config.client_single_channel_size = server_single_channel_size;
        self
    }

    /// Sets the interval for checking the server's heartbeat.
    ///
    /// # Parameters
    ///
    /// - `server_check_heart_interval`: The interval for checking the server's heartbeat.
    ///
    /// # Returns
    ///
    /// The `LynnClientConfigBuilder` instance.
    pub fn with_server_check_heart_interval(
        mut self,
        server_check_heart_interval: &'a u64,
    ) -> Self {
        self.lynn_config.client_check_heart_interval = server_check_heart_interval;
        self
    }

    /// Builds the `LynnClientConfig` instance.
    ///
    /// # Returns
    ///
    /// The `LynnClientConfig` instance.
    pub fn build(self) -> LynnClientConfig<'a> {
        self.lynn_config
    }

    /// Sets the mark for the message header.
    ///
    /// # Parameters
    ///
    /// - `msg_header_mark`: The mark for the message header.
    ///
    /// # Returns
    ///
    /// The `LynnClientConfigBuilder` instance.
    pub(crate) fn with_message_header_mark(mut self, msg_header_mark: &'a u16) -> Self {
        self.lynn_config.message_header_mark = msg_header_mark;
        self
    }

    /// Sets the mark for the message tail.
    ///
    /// # Parameters
    ///
    /// - `msg_tail_mark`: The mark for the message tail.
    ///
    /// # Returns
    ///
    /// The `LynnClientConfigBuilder` instance.
    pub(crate) fn with_message_tail_mark(mut self, msg_tail_mark: &'a u16) -> Self {
        self.lynn_config.message_tail_mark = msg_tail_mark;
        self
    }
}
