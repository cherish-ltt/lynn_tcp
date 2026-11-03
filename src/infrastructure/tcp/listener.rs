use crate::application::server::server_config::LynnServerConfig;

pub(crate) enum TransportListener {
    Tcp(tokio::net::TcpListener),
    #[cfg(feature = "kcp")]
    Kcp(crate::infrastructure::kcp::listener::KcpListener),
}

impl TransportListener {
    pub(crate) async fn bind(config: &LynnServerConfig<'_>) -> crate::Result<Self> {
        #[cfg(feature = "kcp")]
        if let Some(kcp) = &config.kcp {
            kcp.validate()?;
            #[cfg(feature = "tls")]
            if config.get_tls().is_some() {
                return Err(crate::LynnError::config("TLS over KCP is not supported"));
            }
            return Ok(Self::Kcp(
                crate::infrastructure::kcp::listener::KcpListener::bind(
                    &config.get_server_addr(),
                    kcp.clone(),
                )
                .await?,
            ));
        }
        Ok(Self::Tcp(
            tokio::net::TcpListener::bind(config.get_server_addr()).await?,
        ))
    }
}
