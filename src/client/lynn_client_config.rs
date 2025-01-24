use crate::const_config::{
    DEFAULT_CHECK_HEART_INTERVAL, DEFAULT_IPV4, DEFAULT_MESSAGE_HEADER_MARK,
    DEFAULT_MESSAGE_TAIL_MARK, DEFAULT_SYSTEM_CHANNEL_SIZE,
};

#[cfg(feature = "client")]
pub struct LynnClientConfig<'a> {
    // The IPv4 address of the server.
    server_ipv4: &'a str,
    // The size of a single channel.
    client_single_channel_size: &'a usize,
    // The interval for checking heartbeats.
    client_check_heart_interval: &'a u64,
    message_header_mark: &'a u16,
    message_tail_mark: &'a u16,
}

impl<'a> LynnClientConfig<'a> {
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

    pub(crate) fn default() -> Self {
        Self {
            server_ipv4: DEFAULT_IPV4,
            client_single_channel_size: &DEFAULT_SYSTEM_CHANNEL_SIZE,
            client_check_heart_interval: &DEFAULT_CHECK_HEART_INTERVAL,
            message_header_mark: &DEFAULT_MESSAGE_HEADER_MARK,
            message_tail_mark: &DEFAULT_MESSAGE_TAIL_MARK,
        }
    }

    pub(crate) fn get_server_ipv4(&self) -> &str {
        &self.server_ipv4
    }

    pub(crate) fn set_server_ipv4(&mut self, server_ipv4: &'a str) {
        self.server_ipv4 = server_ipv4
    }

    pub(crate) fn get_client_single_channel_size(&self) -> &usize {
        &self.client_single_channel_size
    }

    pub(crate) fn get_server_check_heart_interval(&self) -> &u64 {
        self.client_check_heart_interval
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
pub struct LynnClientConfigBuilder<'a> {
    pub lynn_config: LynnClientConfig<'a>,
}

impl<'a> LynnClientConfigBuilder<'a> {
    pub fn new() -> Self {
        Self {
            lynn_config: LynnClientConfig::default(),
        }
    }

    pub fn with_server_ipv4(mut self, server_ipv4: &'a str) -> Self {
        self.lynn_config.server_ipv4 = server_ipv4;
        self
    }

    pub fn with_server_single_channel_size(
        mut self,
        server_single_channel_size: &'a usize,
    ) -> Self {
        self.lynn_config.client_single_channel_size = server_single_channel_size;
        self
    }

    pub fn with_server_check_heart_interval(
        mut self,
        server_check_heart_interval: &'a u64,
    ) -> Self {
        self.lynn_config.client_check_heart_interval = server_check_heart_interval;
        self
    }

    pub fn build(self) -> LynnClientConfig<'a> {
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
