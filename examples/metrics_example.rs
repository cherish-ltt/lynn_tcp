//! Example of using lynn_tcp with Prometheus metrics
//!
//! Run this example with: cargo run --example metrics_example --features metrics
//!
//! Then visit http://localhost:9091/metrics to see the Prometheus metrics
//! Or visit http://localhost:9091/health for health check

use lynn_tcp::{lynn_server::*, lynn_tcp_dependents::*, lynn_metrics::*};
use tracing_subscriber::fmt;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    fmt::init();

    // Spawn metrics server
    let metrics_config = MetricsServerConfig {
        bind_addr: "0.0.0.0:9091".to_string(),
        enabled: true,
    };

    println!("🚀 Starting Lynn TCP server with metrics...");
    println!("📊 Metrics available at: http://localhost:9091/metrics");
    println!("💚 Health check at: http://localhost:9091/health");
    println!("");

    let _metrics_handle = spawn_metrics_server(metrics_config);

    // Record some initial metrics
    #[cfg(feature = "metrics")]
    {
        METRICS.system.memory_used_bytes.set(128 * 1024 * 1024); // 128MB
        METRICS.system.active_threads.set(4);
    }

    // Start the server
    let _server = LynnServer::new()
        .await
        .add_router(1, my_service)
        .add_router(2, my_service_with_metrics)
        .start()
        .await;

    Ok(())
}

/// Simple handler
pub async fn my_service() -> HandlerResult {
    #[cfg(feature = "metrics")]
    {
        // Track active connections
        METRICS.connections.total.inc();
        METRICS.messages.received_total.inc();
    }

    HandlerResult::new_without_send()
}

/// Handler with metrics tracking
pub async fn my_service_with_metrics(input_buf_vo: InputBufVO) -> HandlerResult {
    #[cfg(feature = "metrics")]
    {
        // Time the handler execution
        let _timer = METRICS.messages.processing_duration_seconds.start_timer();

        // Track message size
        if let Some(data) = input_buf_vo.get_all_bytes().len() {
            METRICS.messages.size_bytes.observe(data as f64);
        }

        METRICS.messages.received_total.inc();
    }

    HandlerResult::new_without_send()
}

/// Example of manually tracking metrics
#[allow(dead_code)]
fn example_manual_metrics() {
    #[cfg(feature = "metrics")]
    {
        // Connection metrics
        METRICS.connections.total.inc();
        METRICS.connections.active.inc();
        METRICS.connections.closed_total.inc();
        METRICS.connections.failed_total.inc();

        // Message metrics
        METRICS.messages.received_total.inc();
        METRICS.messages.sent_total.inc();
        METRICS.messages.invalid_total.inc();
        METRICS.messages.dropped_total.inc();

        // Network metrics
        METRICS.network.bytes_received_total.inc_by(1024);
        METRICS.network.bytes_sent_total.inc_by(2048);

        // System metrics
        METRICS.system.memory_used_bytes.set(256 * 1024 * 1024);
        METRICS.system.active_threads.set(8);
        METRICS.system.queue_size.set(100);

        // Error metrics (with labels)
        METRICS.errors.total
            .with_label_values(&["network_error"])
            .inc();

        METRICS.errors.rate_limit_rejected.inc();
        METRICS.errors.validation_errors.inc();

        // Histogram observation
        METRICS.messages.size_bytes.observe(512.0);
        METRICS.messages.processing_duration_seconds.observe(0.025);
    }
}

/// Example of timing operations
#[allow(dead_code)]
async fn example_timing() {
    #[cfg(feature = "metrics")]
    {
        // Method 1: Manual timing
        let start = std::time::Instant::now();
        // ... do some work ...
        let duration = start.elapsed().as_secs_f64();
        METRICS.messages.processing_duration_seconds.observe(duration);

        // Method 2: Using Timer trait helper
        use lynn_tcp::metrics::Timer;
        METRICS.messages.processing_duration_seconds.observe_duration(|| {
            // ... do some work ...
            println!("Doing work...");
        });
    }
}

/// Export metrics as string (for custom endpoints)
#[allow(dead_code)]
fn export_metrics_text() -> String {
    #[cfg(feature = "metrics")]
    {
        export_metrics()
    }

    #[cfg(not(feature = "metrics"))]
    {
        "# Metrics feature is disabled\n".to_string()
    }
}
