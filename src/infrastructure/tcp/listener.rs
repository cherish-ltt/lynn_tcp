use crate::application::server::server_config::LynnServerConfig;

pub(crate) enum TransportListener {
    Tcp(tokio::net::TcpListener),
    #[cfg(feature = "kcp")]
    Kcp(crate::infrastructure::kcp::listener::KcpListener),
}

impl TransportListener {
    pub(crate) fn local_addr(&self) -> std::io::Result<std::net::SocketAddr> {
        match self {
            Self::Tcp(listener) => listener.local_addr(),
            #[cfg(feature = "kcp")]
            Self::Kcp(listener) => listener.local_addr(),
        }
    }

    pub(crate) async fn bind(config: &LynnServerConfig<'_>) -> crate::Result<Self> {
        #[cfg(feature = "kcp")]
        if let Some(kcp) = &config.kcp {
            kcp.validate()?;
            #[cfg(all(feature = "tls", not(feature = "kcp-tls")))]
            if config.get_tls().is_some() {
                return Err(crate::LynnError::config("TLS over KCP is not supported"));
            }
            let mode = crate::infrastructure::kcp::Mode::Plain;
            #[cfg(feature = "kcp-tls")]
            let mode = if config.get_tls().is_some() {
                crate::infrastructure::kcp::Mode::Tls
            } else {
                mode
            };
            return Ok(Self::Kcp(
                crate::infrastructure::kcp::listener::KcpListener::bind(
                    &config.get_server_addr(),
                    kcp.clone(),
                    mode,
                )
                .await?,
            ));
        }
        Ok(Self::Tcp(
            tokio::net::TcpListener::bind(config.get_server_addr()).await?,
        ))
    }
}
