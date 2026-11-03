#![cfg(feature = "metrics")]

#[test]
fn first_export_registers_zero_metrics_and_preserves_custom_metrics() {
    let text = lynn_tcp::lynn_metrics::export_metrics();
    assert!(text.contains("lynn_connections_active 0\n"));
    assert!(text.contains("lynn_connections_total 0\n"));
    let counter = prometheus::register_counter!("myapp_test_total", "Business events").unwrap();
    counter.inc();
    let text = lynn_tcp::lynn_metrics::export_metrics();
    assert!(text.contains("myapp_test_total 1\n"));
    assert!(text.contains("lynn_connections_total 0\n"));
}
