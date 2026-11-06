//! Client connection lifecycle: connect attempts, automatic reconnection
//! after disconnects, and connection state tracking.
//!
//! The supervisor owns the reconnect policy: after a connection drops (or the
//! initial connect fails) it retries up to `reconnect_max_attempts` times
//! (default 3, one attempt per `reconnect_interval_secs`), then gives up.
//! User-facing channels are created once in `LynnClient::run` and shared with
//! every pump, so they stay valid across reconnections.

use std::time::Duration;

use tokio::{
    net::TcpStream,
    sync::{mpsc, oneshot, watch},
    time,
};
use tracing::{error, info, warn};

use super::connection_state::ConnectionState;

use crate::{
    LynnError, application::client::client_common::run_connection_pumps,
    application::client::client_config::LynnClientConfig,
    domain::model::handler_result::HandlerResult, domain::model::input_buf_vo::InputBufVO,
    infrastructure::tcp::stream::LynnStream,
};

/// Everything the supervisor needs for one connection session.
pub(super) struct ConnectionParams {
    /// The server address ("ip:port").
    addr: String,
    #[cfg(feature = "kcp")]
    kcp: Option<crate::infrastructure::kcp::KcpConfig>,
    /// Per-attempt connect (and TLS handshake) timeout in seconds.
    connect_timeout_secs: u64,
    /// Connect attempts per session (initial connect or after a disconnect).
    reconnect_max_attempts: usize,
    /// Delay in seconds between two attempts.
    reconnect_interval_secs: u64,
    message_header_mark: u16,
    message_tail_mark: u16,
    /// Optional TLS endpoint (connector + resolved server name).
    #[cfg(feature = "tls")]
    tls: Option<crate::infrastructure::tls::tls_provider::ClientTls>,
}

impl ConnectionParams {
    /// Builds owned connection parameters from the client configuration.
    /// Invalid transport configuration fails before any connect attempt.
    pub(super) fn from_config(config: &LynnClientConfig<'_>) -> Result<Self, LynnError> {
        #[cfg(feature = "kcp")]
        if let Some(kcp) = &config.kcp {
            kcp.validate()?;
            #[cfg(all(feature = "tls", not(feature = "kcp-tls")))]
            if config.get_tls().is_some() {
                return Err(LynnError::config("TLS over KCP is not supported"));
            }
        }
        let addr = config.get_server_ipv4();
        Ok(Self {
            addr,
            #[cfg(feature = "kcp")]
            kcp: config.kcp.clone(),
            connect_timeout_secs: (*config.get_connect_timeout_secs()).max(1),
            reconnect_max_attempts: (*config.get_reconnect_max_attempts()).max(1),
            reconnect_interval_secs: *config.get_reconnect_interval_secs(),
            message_header_mark: *config.get_message_header_mark(),
            message_tail_mark: *config.get_message_tail_mark(),
            #[cfg(feature = "tls")]
            tls: match config.get_tls() {
                Some(tls_config) => Some(
                    crate::infrastructure::tls::tls_provider::build_client_tls_mode(
                        tls_config,
                        &config.get_server_ipv4(),
                        {
                            #[cfg(feature = "kcp-tls")]
                            {
                                config.kcp.is_some()
                            }
                            #[cfg(not(feature = "kcp-tls"))]
                            {
                                false
                            }
                        },
                    )?,
                ),
                None => None,
            },
        })
    }
}

/// Performs one connect attempt with the configured transport and handshake.
async fn try_connect(params: &ConnectionParams) -> Result<LynnStream, LynnError> {
    let timeout = Duration::from_secs(params.connect_timeout_secs);
    #[cfg(feature = "kcp")]
    if let Some(config) = &params.kcp {
        use crate::infrastructure::kcp::{Mode, stream::KcpStream};
        let peer = params
            .addr
            .parse()
            .map_err(|e| LynnError::config(format!("invalid KCP address: {e}")))?;
        let attempt = async {
            let mode = Mode::Plain;
            #[cfg(feature = "kcp-tls")]
            let mode = if params.tls.is_some() {
                Mode::Tls
            } else {
                mode
            };
            let stream = KcpStream::connect(peer, config.clone(), mode).await?;
            #[cfg(feature = "kcp-tls")]
            if let Some(tls) = &params.tls {
                let mut stream = tls
                    .connector
                    .connect(tls.server_name.clone(), stream)
                    .await
                    .map_err(|e| LynnError::tls(format!("KCP TLS handshake failed: {e}")))?;
                use tokio::io::AsyncReadExt;
                let mut ready = [0; crate::infrastructure::kcp::TLS_READY.len()];
                stream
                    .read_exact(&mut ready)
                    .await
                    .map_err(|e| LynnError::tls(format!("KCP TLS server readiness failed: {e}")))?;
                if ready != crate::infrastructure::kcp::TLS_READY {
                    return Err(LynnError::tls("invalid KCP TLS server readiness"));
                }
                return Ok(LynnStream::KcpTls(Box::new(stream.into())));
            }
            Ok(LynnStream::Kcp(stream))
        };
        return time::timeout(timeout, attempt)
            .await
            .map_err(|_| LynnError::timeout("KCP connection attempt timed out"))?;
    }

    let tcp = time::timeout(timeout, TcpStream::connect(&params.addr))
        .await
        .map_err(|_| {
            LynnError::timeout(format!(
                "connect to {} timed out after {}s",
                params.addr, params.connect_timeout_secs
            ))
        })??;
    #[cfg(feature = "tls")]
    if let Some(tls) = &params.tls {
        let tls_stream =
            time::timeout(timeout, tls.connector.connect(tls.server_name.clone(), tcp))
                .await
                .map_err(|_| {
                    LynnError::timeout(format!(
                        "TLS handshake with {} timed out after {}s",
                        params.addr, params.connect_timeout_secs
                    ))
                })?
                .map_err(|e| {
                    LynnError::tls(format!("TLS handshake with {} failed: {e}", params.addr))
                })?;
        return Ok(LynnStream::Tls(Box::new(tls_stream.into())));
    }
    Ok(LynnStream::Plain(tcp))
}

/// Discards messages queued while the connection was down (e.g. heartbeats),
/// so senders are not back-pressured by undeliverable frames.
fn drain_write_queue(rx_write: &mut mpsc::Receiver<HandlerResult>) -> usize {
    let mut dropped = 0;
    while let Ok(message) = rx_write.try_recv() {
        drop(message);
        dropped += 1;
    }
    dropped
}

/// The connection supervisor: keeps exactly one live connection to the
/// server, reconnecting up to `reconnect_max_attempts` times per session.
///
/// `init_tx` reports the outcome of the initial connect session (resolved
/// once, on first success or after all initial attempts fail).
pub(super) async fn connection_supervisor(
    params: ConnectionParams,
    tx_read: mpsc::Sender<InputBufVO>,
    mut rx_write: mpsc::Receiver<HandlerResult>,
    state_tx: watch::Sender<ConnectionState>,
    mut init_tx: Option<oneshot::Sender<Result<(), String>>>,
) {
    let max_attempts = params.reconnect_max_attempts;
    let mut attempts: usize = 0;
    let mut generation = 0;
    loop {
        // If the client itself is gone (its receive half dropped), stop.
        if tx_read.is_closed() {
            info!("Client - [connection supervisor] client dropped, exiting");
            break;
        }
        match try_connect(&params).await {
            Ok(stream) => {
                attempts = 0;
                generation += 1;
                // Publish transport readiness before resolving the initial start().
                state_tx.send_replace(ConnectionState::Connected { generation });
                if let Some(init) = init_tx.take() {
                    let _ = init.send(Ok(()));
                }
                info!(
                    "Client - [connection supervisor] connected to [server_addr:{}]",
                    params.addr
                );

                run_connection_pumps(
                    stream,
                    params.message_header_mark,
                    params.message_tail_mark,
                    tx_read.clone(),
                    &mut rx_write,
                )
                .await;

                state_tx.send_replace(ConnectionState::Reconnecting { generation });
                let dropped = drain_write_queue(&mut rx_write);
                if dropped > 0 {
                    warn!(
                        "Client - [connection supervisor] dropped {} stale queued messages after disconnect",
                        dropped
                    );
                }
                warn!(
                    "Client - [connection supervisor] connection lost, reconnecting to {} (up to {} attempts)",
                    params.addr, max_attempts
                );
            },
            Err(e) => {
                attempts += 1;
                if attempts >= max_attempts {
                    let message = format!(
                        "connect to server {} failed after {} attempts: {}",
                        params.addr, max_attempts, e
                    );
                    if let Some(init) = init_tx.take() {
                        let _ = init.send(Err(message.clone()));
                    }
                    error!("Client - [connection supervisor] {}", message);
                    break;
                }
                warn!(
                    "Client - [connection supervisor] connect attempt {}/{} failed: {}",
                    attempts, max_attempts, e
                );
            },
        }
        time::sleep(Duration::from_secs(params.reconnect_interval_secs)).await;
    }
    drop(rx_write);
    drop(tx_read);
    state_tx.send_replace(ConnectionState::Stopped { generation });
}
