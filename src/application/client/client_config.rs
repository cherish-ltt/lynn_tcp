use std::net::{SocketAddr, ToSocketAddrs};

use crate::const_config::{
    DEFAULT_ADDR, DEFAULT_CHECK_HEART_INTERVAL, DEFAULT_CONNECT_TIMEOUT_SECS,
    DEFAULT_MESSAGE_HEADER_MARK, DEFAULT_MESSAGE_TAIL_MARK, DEFAULT_RECONNECT_INTERVAL_SECS,
    DEFAULT_RECONNECT_MAX_ATTEMPTS, DEFAULT_SYSTEM_CHANNEL_SIZE,
};
use crate::{LynnError, Result};

/// The configuration for the Lynn client.
///
/// This struct holds the configuration options for the Lynn client, including the server's IPv4 address,
/// the size of the client's single channel, the interval for checking the server's heartbeat, and the marks
/// for the message header and tail.
#[cfg(feature = "client")]
pub struct LynnClientConfig<'a> {
    /// The address of the server.
    server_addr: SocketAddr,
    /// The size of the client's single channel.
    client_single_channel_size: &'a usize,
    /// The interval for checking the server's heartbeat.
    client_check_heart_interval: &'a u64,
    /// The mark for the message header.
    message_header_mark: &'a u16,
    /// The mark for the message tail.
    message_tail_mark: &'a u16,
    /// Connect attempts per connection session (initial connect and each
    /// automatic reconnect). Default: 3.
    reconnect_max_attempts: &'a usize,
    /// Delay in seconds between two connect attempts. Default: 1.
    reconnect_interval_secs: &'a u64,
    /// Per-attempt connect (and TLS handshake) timeout in seconds. Default: 3.
    connect_timeout_secs: &'a u64,
    /// Optional TLS 1.3 configuration. `None` keeps TLS disabled (default).
    #[cfg(feature = "tls")]
    tls: Option<crate::infrastructure::tls::tls_config::TlsClientConfig>,
}

impl<'a> LynnClientConfig<'a> {
    /// Returns the default configuration for the Lynn client.
    ///
    /// # Returns
    ///
    /// The default `LynnClientConfig` instance.
    pub(crate) fn default() -> Self {
        Self {
            server_addr: *DEFAULT_ADDR,
            client_single_channel_size: &DEFAULT_SYSTEM_CHANNEL_SIZE,
            client_check_heart_interval: &DEFAULT_CHECK_HEART_INTERVAL,
            message_header_mark: &DEFAULT_MESSAGE_HEADER_MARK,
            message_tail_mark: &DEFAULT_MESSAGE_TAIL_MARK,
            reconnect_max_attempts: &DEFAULT_RECONNECT_MAX_ATTEMPTS,
            reconnect_interval_secs: &DEFAULT_RECONNECT_INTERVAL_SECS,
            connect_timeout_secs: &DEFAULT_CONNECT_TIMEOUT_SECS,
            #[cfg(feature = "tls")]
            tls: None,
        }
    }

    /// Returns the server's IPv4 address.
    ///
    /// # Returns
    ///
    /// The server's IPv4 address.
    pub(crate) fn get_server_ipv4(&self) -> String {
        self.server_addr.to_string()
    }

    /// Returns the size of the client's single channel.
    ///
    /// # Returns
    ///
    /// The size of the client's single channel.
    pub(crate) fn get_client_single_channel_size(&self) -> &usize {
        self.client_single_channel_size
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

    /// Returns the connect attempts per connection session.
    pub(crate) fn get_reconnect_max_attempts(&self) -> &usize {
        self.reconnect_max_attempts
    }

    /// Returns the delay in seconds between two connect attempts.
    pub(crate) fn get_reconnect_interval_secs(&self) -> &u64 {
        self.reconnect_interval_secs
    }

    /// Returns the per-attempt connect timeout in seconds.
    pub(crate) fn get_connect_timeout_secs(&self) -> &u64 {
        self.connect_timeout_secs
    }

    /// Returns the optional TLS configuration (feature `tls`).
    ///
    /// # Returns
    ///
    /// `Some(&TlsClientConfig)` when TLS is enabled, `None` otherwise.
    #[cfg(feature = "tls")]
    pub(crate) fn get_tls(
        &self,
    ) -> Option<&crate::infrastructure::tls::tls_config::TlsClientConfig> {
        self.tls.as_ref()
    }
}

/// A builder for creating `LynnClientConfig` instances.
///
/// This struct provides a fluent interface for setting the configuration options for the Lynn client.
#[cfg(feature = "client")]
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
    #[deprecated(since = "1.1.7", note = "use `with_server_addr` instead")]
    pub fn with_server_ipv4<T>(mut self, server_addr: T) -> Result<Self>
    where
        T: ToSocketAddrs,
    {
        self.lynn_config.server_addr = server_addr
            .to_socket_addrs()
            .map_err(|e| LynnError::invalid_address(format!("Failed to parse address: {}", e)))?
            .next()
            .ok_or_else(|| LynnError::invalid_address("No addresses found"))?;
        Ok(self)
    }

    pub fn with_server_addr<T>(mut self, server_addr: T) -> Result<Self>
    where
        T: ToSocketAddrs,
    {
        self.lynn_config.server_addr = server_addr
            .to_socket_addrs()
            .map_err(|e| LynnError::invalid_address(format!("Failed to parse address: {}", e)))?
            .next()
            .ok_or_else(|| LynnError::invalid_address("No addresses found"))?;
        Ok(self)
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
    pub fn with_message_header_mark(mut self, msg_header_mark: &'a u16) -> Self {
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
    pub fn with_message_tail_mark(mut self, msg_tail_mark: &'a u16) -> Self {
        self.lynn_config.message_tail_mark = msg_tail_mark;
        self
    }

    /// Enables TLS 1.3 with the given client-side TLS configuration
    /// (requires the `tls` feature). TLS stays disabled unless this is called.
    ///
    /// # Parameters
    ///
    /// - `tls`: The client TLS configuration (CA trust anchor, optional SNI
    ///   name and optional client certificate for mutual TLS).
    ///
    /// # Returns
    ///
    /// The `LynnClientConfigBuilder` instance.
    #[cfg(feature = "tls")]
    pub fn with_tls(
        mut self,
        tls: crate::infrastructure::tls::tls_config::TlsClientConfig,
    ) -> Self {
        self.lynn_config.tls = Some(tls);
        self
    }

    /// Sets how many connect attempts are made per connection session: the
    /// initial connect and every automatic reconnect after a disconnect.
    /// Minimum effective value is 1.
    ///
    /// # Parameters
    ///
    /// - `reconnect_max_attempts`: Attempts per session. Default: 3.
    ///
    /// # Returns
    ///
    /// The `LynnClientConfigBuilder` instance.
    pub fn with_reconnect_max_attempts(mut self, reconnect_max_attempts: &'a usize) -> Self {
        self.lynn_config.reconnect_max_attempts = reconnect_max_attempts;
        self
    }

    /// Sets the delay in seconds between two connect attempts.
    ///
    /// # Parameters
    ///
    /// - `reconnect_interval_secs`: Delay in seconds. Default: 1.
    ///
    /// # Returns
    ///
    /// The `LynnClientConfigBuilder` instance.
    pub fn with_reconnect_interval_secs(mut self, reconnect_interval_secs: &'a u64) -> Self {
        self.lynn_config.reconnect_interval_secs = reconnect_interval_secs;
        self
    }

    /// Sets the per-attempt connect (and TLS handshake) timeout in seconds.
    ///
    /// # Parameters
    ///
    /// - `connect_timeout_secs`: Timeout in seconds. Default: 3.
    ///
    /// # Returns
    ///
    /// The `LynnClientConfigBuilder` instance.
    pub fn with_connect_timeout_secs(mut self, connect_timeout_secs: &'a u64) -> Self {
        self.lynn_config.connect_timeout_secs = connect_timeout_secs;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    static CHANNEL: usize = 32;
    static HEART: u64 = 3;
    static HEADER: u16 = 0x1122;
    static TAIL: u16 = 0x3344;
    static ATTEMPTS: usize = 5;
    static INTERVAL: u64 = 7;
    static CONNECT_TIMEOUT: u64 = 9;

    #[test]
    fn builder_sets_every_field() {
        let cfg = LynnClientConfigBuilder::new()
            .with_server_addr("127.0.0.1:9999")
            .expect("valid addr")
            .with_server_single_channel_size(&CHANNEL)
            .with_server_check_heart_interval(&HEART)
            .with_message_header_mark(&HEADER)
            .with_message_tail_mark(&TAIL)
            .with_reconnect_max_attempts(&ATTEMPTS)
            .with_reconnect_interval_secs(&INTERVAL)
            .with_connect_timeout_secs(&CONNECT_TIMEOUT)
            .build();

        assert_eq!(cfg.get_server_ipv4(), "127.0.0.1:9999");
        assert_eq!(cfg.get_client_single_channel_size(), &CHANNEL);
        assert_eq!(cfg.get_server_check_heart_interval(), &HEART);
        assert_eq!(cfg.get_message_header_mark(), &HEADER);
        assert_eq!(cfg.get_message_tail_mark(), &TAIL);
        assert_eq!(cfg.get_reconnect_max_attempts(), &ATTEMPTS);
        assert_eq!(cfg.get_reconnect_interval_secs(), &INTERVAL);
        assert_eq!(cfg.get_connect_timeout_secs(), &CONNECT_TIMEOUT);
    }

    #[test]
    fn defaults_enable_three_reconnect_attempts() {
        let cfg = LynnClientConfig::default();
        assert_eq!(*cfg.get_reconnect_max_attempts(), 3);
        assert_eq!(*cfg.get_reconnect_interval_secs(), 1);
        assert_eq!(*cfg.get_connect_timeout_secs(), 3);
    }

    #[test]
    fn invalid_addr_is_rejected() {
        assert!(
            LynnClientConfigBuilder::new()
                .with_server_addr("??")
                .is_err()
        );
    }

    #[test]
    #[allow(deprecated)]
    fn deprecated_addr_setter_still_applies() {
        let cfg = LynnClientConfigBuilder::new()
            .with_server_ipv4("127.0.0.1:9998")
            .expect("valid addr")
            .build();
        assert_eq!(cfg.get_server_ipv4(), "127.0.0.1:9998");
    }
}
