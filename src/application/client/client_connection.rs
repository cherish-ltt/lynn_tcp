//! Client connection lifecycle: connect attempts, automatic reconnection
//! after disconnects, and connection state tracking.
//!
//! The supervisor owns the reconnect policy: after a connection drops (or the
//! initial connect fails) it retries up to `reconnect_max_attempts` times
//! (default 3, one attempt per `reconnect_interval_secs`), then gives up.
//! User-facing channels are created once in `LynnClient::run` and shared with
//! every pump, so they stay valid across reconnections.

use std::{sync::Arc, time::Duration};

use tokio::{
    net::TcpStream,
    sync::{Mutex, mpsc, oneshot, watch},
    time,
};
use tracing::{error, info, warn};

use crate::{
    LynnError,
    application::client::client_common::{SharedWriteReceiver, spawn_connection_pumps},
    application::client::client_config::LynnClientConfig,
    domain::model::input_buf_vo::InputBufVO,
    infrastructure::tcp::stream::LynnStream,
};

/// Everything the supervisor needs for one connection session.
pub(super) struct ConnectionParams {
    /// The server address ("ip:port").
    pub(super) addr: String,
    /// Per-attempt connect (and TLS handshake) timeout in seconds.
    pub(super) connect_timeout_secs: u64,
    /// Connect attempts per session (initial connect or after a disconnect).
    pub(super) reconnect_max_attempts: usize,
    /// Delay in seconds between two attempts.
    pub(super) reconnect_interval_secs: u64,
    pub(super) message_header_mark: u16,
    pub(super) message_tail_mark: u16,
    /// Optional TLS endpoint (connector + resolved server name).
    #[cfg(feature = "tls")]
    pub(super) tls: Option<crate::infrastructure::tls::tls_provider::ClientTls>,
}

impl ConnectionParams {
    /// Builds owned connection parameters from the client configuration.
    /// A bad TLS configuration fails fast here, before any connect attempt.
    pub(super) fn from_config(config: &LynnClientConfig<'_>) -> Result<Self, LynnError> {
        let addr = config.get_server_ipv4();
        Ok(Self {
            addr,
            connect_timeout_secs: (*config.get_connect_timeout_secs()).max(1),
            reconnect_max_attempts: (*config.get_reconnect_max_attempts()).max(1),
            reconnect_interval_secs: *config.get_reconnect_interval_secs(),
            message_header_mark: *config.get_message_header_mark(),
            message_tail_mark: *config.get_message_tail_mark(),
            #[cfg(feature = "tls")]
            tls: match config.get_tls() {
                Some(tls_config) => {
                    Some(crate::infrastructure::tls::tls_provider::build_client_tls(
                        tls_config,
                        &config.get_server_ipv4(),
                    )?)
                },
                None => None,
            },
        })
    }
}

/// Performs one connect attempt (TCP, plus TLS handshake when configured).
async fn try_connect(params: &ConnectionParams) -> Result<LynnStream, LynnError> {
    let timeout = Duration::from_secs(params.connect_timeout_secs);
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
async fn drain_write_queue(rx_write: &SharedWriteReceiver) -> usize {
    let mut dropped = 0;
    let mut guard = rx_write.lock().await;
    while let Ok(message) = guard.try_recv() {
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
    rx_write: SharedWriteReceiver,
    state_tx: watch::Sender<bool>,
    mut init_tx: Option<oneshot::Sender<Result<(), String>>>,
) {
    let max_attempts = params.reconnect_max_attempts;
    let mut attempts: usize = 0;
    loop {
        // If the client itself is gone (its receive half dropped), stop.
        if tx_read.is_closed() {
            info!("Client - [connection supervisor] client dropped, exiting");
            state_tx.send_replace(false);
            return;
        }
        match try_connect(&params).await {
            Ok(stream) => {
                attempts = 0;
                // Publish the connected state BEFORE resolving the start()
                // notification, so `is_connected()` is guaranteed to observe
                // `true` as soon as `start()` returns.
                state_tx.send_replace(true);
                if let Some(init) = init_tx.take() {
                    let _ = init.send(Ok(()));
                }
                info!(
                    "Client - [connection supervisor] connected to [server_addr:{}]",
                    params.addr
                );

                let (_close_tx, close_rx) = watch::channel(false);
                let read_handle = spawn_connection_pumps(
                    stream,
                    params.message_header_mark,
                    params.message_tail_mark,
                    tx_read.clone(),
                    rx_write.clone(),
                    close_rx,
                );
                let _ = read_handle.await;

                state_tx.send_replace(false);
                _close_tx.send_replace(true);
                let dropped = drain_write_queue(&rx_write).await;
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
                    state_tx.send_replace(false);
                    error!("Client - [connection supervisor] {}", message);
                    return;
                }
                warn!(
                    "Client - [connection supervisor] connect attempt {}/{} failed: {}",
                    attempts, max_attempts, e
                );
                time::sleep(Duration::from_secs(params.reconnect_interval_secs)).await;
            },
        }
    }
}
