//! Metrics collection for lynn_tcp framework
//!
//! This module provides Prometheus metrics for monitoring the TCP server.
//! It includes metrics for connections, messages, network traffic, errors, and system resources.

#[cfg(feature = "metrics")]
use lazy_static::lazy_static;
#[cfg(feature = "metrics")]
use prometheus::{
    Counter, Gauge, Histogram, HistogramOpts, TextEncoder, histogram_opts, register_counter,
    register_gauge, register_histogram,
};

/// Connection metrics
#[cfg(feature = "metrics")]
pub struct ConnectionMetrics {
    /// Total number of connections established
    pub total: Counter,
    /// Current number of active connections
    pub active: Gauge,
    /// Total number of connections closed
    pub closed_total: Counter,
    /// Total number of failed connection attempts
    pub failed_total: Counter,
}

/// Message metrics
#[cfg(feature = "metrics")]
pub struct MessageMetrics {
    /// Total number of messages received
    pub received_total: Counter,
    /// Total number of messages sent
    pub sent_total: Counter,
    /// Total number of invalid messages received
    pub invalid_total: Counter,
    /// Total number of dropped messages
    pub dropped_total: Counter,
    /// Message size histogram in bytes
    pub size_bytes: Histogram,
    /// Message processing duration histogram
    pub processing_duration_seconds: Histogram,
}

/// Network traffic metrics
#[cfg(feature = "metrics")]
pub struct NetworkMetrics {
    /// Total bytes received
    pub bytes_received_total: Counter,
    /// Total bytes sent
    pub bytes_sent_total: Counter,
    /// TCP retransmits
    pub tcp_retransmits_total: Counter,
    /// TCP errors
    pub tcp_errors_total: Counter,
}

/// System metrics
#[cfg(feature = "metrics")]
pub struct SystemMetrics {
    /// Memory usage in bytes
    pub memory_used_bytes: Gauge,
    /// Active thread pool threads
    pub active_threads: Gauge,
    /// Thread pool queue size
    pub queue_size: Gauge,
}

/// All metrics grouped together
#[cfg(feature = "metrics")]
pub struct Metrics {
    pub connections: ConnectionMetrics,
    pub messages: MessageMetrics,
    pub network: NetworkMetrics,
    pub system: SystemMetrics,
}

#[cfg(feature = "metrics")]
lazy_static! {
    /// Global metrics instance
    pub static ref METRICS: Metrics = {
        // Connection metrics
        let connections_total = register_counter!(
            "lynn_connections_total", "Total number of connections established"
        ).unwrap();

        let connections_active = register_gauge!(
            "lynn_connections_active", "Current number of active connections"
        ).unwrap();

        let connections_closed_total = register_counter!(
            "lynn_connections_closed_total", "Total number of connections closed"
        ).unwrap();

        let connections_failed_total = register_counter!(
            "lynn_connections_failed_total", "Total number of failed connection attempts"
        ).unwrap();

        // Message metrics
        let messages_received_total = register_counter!(
            "lynn_messages_received_total", "Total number of messages received"
        ).unwrap();

        let messages_sent_total = register_counter!(
            "lynn_messages_sent_total", "Total number of messages sent"
        ).unwrap();

        let messages_invalid_total = register_counter!(
            "lynn_messages_invalid_total", "Total number of invalid messages received"
        ).unwrap();

        let messages_dropped_total = register_counter!(
            "lynn_messages_dropped_total", "Total number of dropped messages"
        ).unwrap();

        let message_size_bytes_opts = histogram_opts!(
            "lynn_message_size_bytes",
            "Message size in bytes"
        ).buckets(vec![0.0, 128.0, 512.0, 2048.0, 10240.0, 102400.0, 1048576.0]);


        let message_size_bytes = register_histogram!(message_size_bytes_opts).unwrap();

        let message_processing_duration_opts = histogram_opts!(
            "lynn_message_processing_duration_seconds",
            "Message processing duration in seconds"
        ).buckets(vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.5, 1.0, 5.0]);


        let message_processing_duration = register_histogram!(message_processing_duration_opts).unwrap();

        // Network metrics
        let network_bytes_received_total = register_counter!(
            "lynn_network_bytes_received_total", "Total bytes received"
        ).unwrap();

        let network_bytes_sent_total = register_counter!(
            "lynn_network_bytes_sent_total", "Total bytes sent"
        ).unwrap();

        let network_tcp_retransmits_total = register_counter!(
            "lynn_network_tcp_retransmits_total", "Total TCP retransmits"
        ).unwrap();

        let network_tcp_errors_total = register_counter!(
            "lynn_network_tcp_errors_total", "Total TCP errors"
        ).unwrap();

        // System metrics
        let system_memory_used_bytes = register_gauge!(
            "lynn_system_memory_used_bytes", "Memory usage in bytes"
        ).unwrap();

        let system_active_threads = register_gauge!(
            "lynn_system_active_threads", "Number of active threads"
        ).unwrap();

        let system_queue_size = register_gauge!(
            "lynn_system_queue_size", "Thread pool queue size"
        ).unwrap();

        Metrics {
            connections: ConnectionMetrics {
                total: connections_total,
                active: connections_active,
                closed_total: connections_closed_total,
                failed_total: connections_failed_total,
            },
            messages: MessageMetrics {
                received_total: messages_received_total,
                sent_total: messages_sent_total,
                invalid_total: messages_invalid_total,
                dropped_total: messages_dropped_total,
                size_bytes: message_size_bytes,
                processing_duration_seconds: message_processing_duration,
            },
            network: NetworkMetrics {
                bytes_received_total: network_bytes_received_total,
                bytes_sent_total: network_bytes_sent_total,
                tcp_retransmits_total: network_tcp_retransmits_total,
                tcp_errors_total: network_tcp_errors_total,
            },
            system: SystemMetrics {
                memory_used_bytes: system_memory_used_bytes,
                active_threads: system_active_threads,
                queue_size: system_queue_size,
            },
        }
    };
}

/// Exports metrics in Prometheus text format
#[cfg(feature = "metrics")]
pub fn export_metrics() -> String {
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    match encoder.encode_to_string(&metric_families) {
        Ok(text) => text,
        Err(e) => format!("# Error encoding metrics: {}", e),
    }
}

/// Helper trait for timing operations
#[cfg(feature = "metrics")]
pub trait Timer {
    fn observe_duration<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R;
}

#[cfg(feature = "metrics")]
impl Timer for Histogram {
    fn observe_duration<F, R>(&self, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let start = std::time::Instant::now();
        let result = f();
        let duration = start.elapsed().as_secs_f64();
        self.observe(duration);
        result
    }
}

#[cfg(not(feature = "metrics"))]
/// No-op metrics when feature is disabled
pub struct Metrics;

#[cfg(not(feature = "metrics"))]
impl Metrics {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(test)]
#[cfg(feature = "metrics")]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_export() {
        // Increment some metrics
        METRICS.connections.total.inc();
        METRICS.connections.active.inc();
        METRICS.messages.received_total.inc();

        // Export and check format
        let metrics_text = export_metrics();
        assert!(metrics_text.contains("lynn_connections_total"));
        assert!(metrics_text.contains("lynn_connections_active"));
        assert!(metrics_text.contains("lynn_messages_received_total"));
    }

    #[test]
    fn test_histogram_observe() {
        METRICS.messages.size_bytes.observe(1024.0);
        METRICS.messages.size_bytes.observe(2048.0);

        let metrics_text = export_metrics();
        assert!(metrics_text.contains("lynn_message_size_bytes"));
        assert!(metrics_text.contains("1024"));
    }

    #[test]
    fn test_timer_helper() {
        use std::time::Duration;
        let timer = &METRICS.messages.processing_duration_seconds;

        timer.observe_duration(|| {
            std::thread::sleep(Duration::from_millis(10));
        });

        let metrics_text = export_metrics();
        assert!(metrics_text.contains("lynn_message_processing_duration_seconds"));
    }
}
