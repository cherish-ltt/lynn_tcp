//! HTTP metrics endpoint for Prometheus scraping
//!
//! This module provides a simple HTTP server that exposes metrics
//! at the /metrics endpoint for Prometheus to scrape.

#[cfg(feature = "metrics")]
use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(feature = "metrics")]
use tokio::net::TcpListener;
#[cfg(feature = "metrics")]
use tokio::time::{Duration, timeout};
#[cfg(feature = "metrics")]
use tracing::{error, info};

use crate::infrastructure::metrics::metrics::export_metrics;

/// Serves metrics HTTP endpoint
///
/// This function starts a simple HTTP server that responds to
/// GET /metrics requests with Prometheus-formatted metrics.
///
/// # Arguments
///
/// * `bind_addr` - The address to bind the metrics server to
///
/// # Returns
///
/// * `Ok(())` if the server started successfully
/// * `Err(Box<dyn std::error::Error>)` if there was an error
#[cfg(feature = "metrics")]
pub async fn serve_metrics(bind_addr: &str) -> Result<(), Box<dyn std::error::Error>> {
    let listener = TcpListener::bind(bind_addr).await?;
    let local_addr = listener.local_addr()?;

    info!("Metrics server listening on http://{}", local_addr);

    loop {
        match listener.accept().await {
            Ok((mut socket, addr)) => {
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(&mut socket).await {
                        error!("Error handling connection from {}: {}", addr, e);
                    }
                });
            },
            Err(e) => {
                error!("Error accepting connection: {}", e);
            },
        }
    }
}

/// Handles a single HTTP connection
#[cfg(feature = "metrics")]
async fn handle_connection(
    socket: &mut tokio::net::TcpStream,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut read_buf = [0; 1024];

    // Read the request line
    let line_len = timeout(Duration::from_secs(5), async {
        let mut filled = 0;
        loop {
            if filled == read_buf.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "metrics request line exceeds 1024 bytes",
                ));
            }
            let n = socket.read(&mut read_buf[filled..]).await?;
            if n == 0 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "incomplete metrics request line",
                ));
            }
            filled += n;
            if let Some(end) = read_buf[..filled]
                .windows(2)
                .position(|pair| pair == b"\r\n")
            {
                return Ok(end);
            }
        }
    })
    .await??;
    let request_line = std::str::from_utf8(&read_buf[..line_len])?;

    // Check if it's a GET /metrics request
    if request_line.starts_with("GET /metrics ") {
        let metrics_data = export_metrics();

        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\n\r\n{}",
            metrics_data.len(),
            metrics_data
        );

        socket.write_all(response.as_bytes()).await?;
    } else if request_line.starts_with("GET /health ") {
        // Health check endpoint
        let response = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK";
        socket.write_all(response.as_bytes()).await?;
    } else {
        // 404 for other paths
        let response = "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 9\r\n\r\nNot Found";
        socket.write_all(response.as_bytes()).await?;
    }

    Ok(())
}

/// Configuration for the metrics server
#[derive(Clone, Debug)]
pub struct MetricsServerConfig {
    /// Address to bind the metrics server to
    pub bind_addr: String,
    /// Whether to enable the metrics server
    pub enabled: bool,
}

impl Default for MetricsServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "0.0.0.0:9091".to_string(),
            enabled: true,
        }
    }
}

/// Starts the metrics server in the background
///
/// # Arguments
///
/// * `config` - Configuration for the metrics server
///
/// # Returns
///
/// A tokio::JoinHandle for the server task
#[cfg(feature = "metrics")]
pub fn spawn_metrics_server(config: MetricsServerConfig) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        if config.enabled {
            if let Err(e) = serve_metrics(&config.bind_addr).await {
                error!("Metrics server error: {}", e);
            }
        }
    })
}

#[cfg(test)]
#[cfg(feature = "metrics")]
mod tests {
    use super::*;
    use tokio::time::{Duration, sleep};

    #[tokio::test]
    async fn fragmented_request_lines_are_accepted_at_every_boundary() {
        for path in ["metrics", "health"] {
            let request = format!("GET /{path} HTTP/1.1\r\n\r\n");
            for split in 1..request.len() {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
                    .await
                    .unwrap();
                let (mut server, _) = listener.accept().await.unwrap();
                let (result, response) = tokio::join!(handle_connection(&mut server), async {
                    client
                        .write_all(&request.as_bytes()[..split])
                        .await
                        .unwrap();
                    sleep(Duration::from_millis(5)).await;
                    client
                        .write_all(&request.as_bytes()[split..])
                        .await
                        .unwrap();
                    let mut response = [0; 17];
                    timeout(Duration::from_secs(2), client.read_exact(&mut response))
                        .await
                        .unwrap()
                        .unwrap();
                    response
                });
                result.unwrap();
                assert!(
                    response.starts_with(b"HTTP/1.1 200 OK\r\n"),
                    "{path}, split={split}: {}",
                    String::from_utf8_lossy(&response)
                );
            }
        }
    }

    #[tokio::test]
    async fn request_line_can_arrive_one_byte_at_a_time() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        let (result, ()) = tokio::join!(handle_connection(&mut server), async {
            for byte in b"GET /health HTTP/1.1\r\n" {
                client.write_all(&[*byte]).await.unwrap();
                sleep(Duration::from_millis(5)).await;
            }
        });
        result.unwrap();
        let mut response = [0; 17];
        timeout(Duration::from_secs(2), client.read_exact(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&response, b"HTTP/1.1 200 OK\r\n");
    }

    #[tokio::test]
    async fn incomplete_and_oversized_request_lines_are_rejected() {
        for (request, expected) in [
            (b"GET /met".to_vec(), std::io::ErrorKind::UnexpectedEof),
            (vec![b'G'; 1024], std::io::ErrorKind::InvalidData),
        ] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (mut server, _) = listener.accept().await.unwrap();
            client.write_all(&request).await.unwrap();
            client.shutdown().await.unwrap();
            let error = handle_connection(&mut server).await.unwrap_err();
            assert_eq!(
                error.downcast_ref::<std::io::Error>().unwrap().kind(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn incomplete_request_line_has_a_total_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (mut server, _) = listener.accept().await.unwrap();
        let (result, ()) = tokio::join!(
            timeout(Duration::from_secs(7), handle_connection(&mut server)),
            async {
                for _ in 0..6 {
                    client.write_all(b"G").await.unwrap();
                    sleep(Duration::from_secs(1)).await;
                }
            }
        );
        assert!(
            result
                .expect("incoming bytes must not reset the total deadline")
                .unwrap_err()
                .is::<tokio::time::error::Elapsed>()
        );
    }

    #[tokio::test]
    async fn test_metrics_server() {
        lazy_static::initialize(&crate::infrastructure::metrics::metrics::METRICS);
        let config = MetricsServerConfig {
            bind_addr: "127.0.0.1:19991".to_string(),
            enabled: true,
        };

        let handle = spawn_metrics_server(config);

        // Give the server time to start
        sleep(Duration::from_millis(100)).await;

        // Test the /metrics endpoint
        let mut stream = tokio::net::TcpStream::connect("127.0.0.1:19991")
            .await
            .unwrap();
        stream
            .write_all(b"GET /metrics HTTP/1.1\r\n\r\n")
            .await
            .unwrap();

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();

        let response_str = String::from_utf8(response).unwrap();
        assert!(response_str.contains("HTTP/1.1 200 OK"));
        assert!(response_str.contains("lynn_"));

        handle.abort();
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let config = MetricsServerConfig {
            bind_addr: "127.0.0.1:19992".to_string(),
            enabled: true,
        };

        let handle = spawn_metrics_server(config);

        sleep(Duration::from_millis(100)).await;

        let mut stream = tokio::net::TcpStream::connect("127.0.0.1:19992")
            .await
            .unwrap();
        stream
            .write_all(b"GET /health HTTP/1.1\r\n\r\n")
            .await
            .unwrap();

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();

        let response_str = String::from_utf8(response).unwrap();
        assert!(response_str.contains("HTTP/1.1 200 OK"));
        assert!(response_str.contains("OK"));

        handle.abort();
    }

    #[tokio::test]
    async fn test_404_endpoint() {
        let config = MetricsServerConfig {
            bind_addr: "127.0.0.1:19993".to_string(),
            enabled: true,
        };

        let handle = spawn_metrics_server(config);

        sleep(Duration::from_millis(100)).await;

        let mut stream = tokio::net::TcpStream::connect("127.0.0.1:19993")
            .await
            .unwrap();
        stream
            .write_all(b"GET /unknown HTTP/1.1\r\n\r\n")
            .await
            .unwrap();

        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.unwrap();

        let response_str = String::from_utf8(response).unwrap();
        assert!(response_str.contains("HTTP/1.1 404 Not Found"));

        handle.abort();
    }
}
