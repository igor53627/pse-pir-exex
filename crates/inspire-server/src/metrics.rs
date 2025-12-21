//! Prometheus metrics for PIR server
//!
//! Privacy-safe metrics: only lane and outcome labels, never query content.

use metrics::{counter, gauge, histogram};
use std::time::Duration;

pub const LANE_HOT: &str = "hot";
pub const LANE_COLD: &str = "cold";
pub const LANE_BALANCES: &str = "balances";

pub const OUTCOME_OK: &str = "ok";
pub const OUTCOME_CLIENT_ERROR: &str = "client_error";
pub const OUTCOME_SERVER_ERROR: &str = "server_error";

pub fn record_pir_request(lane: &str, outcome: &str, duration: Duration) {
    counter!("pir_requests_total", "lane" => lane.to_string(), "outcome" => outcome.to_string()).increment(1);
    histogram!("pir_request_duration_seconds", "lane" => lane.to_string(), "outcome" => outcome.to_string())
        .record(duration.as_secs_f64());
}

pub fn record_pir_request_start(lane: &str) {
    gauge!("pir_requests_in_flight", "lane" => lane.to_string()).increment(1.0);
}

pub fn record_pir_request_end(lane: &str) {
    gauge!("pir_requests_in_flight", "lane" => lane.to_string()).decrement(1.0);
}

pub fn set_lane_loaded(lane: &str, loaded: bool) {
    gauge!("pir_lane_loaded", "lane" => lane.to_string()).set(if loaded { 1.0 } else { 0.0 });
}

pub fn set_lane_block_number(lane: &str, block: u64) {
    gauge!("pir_lane_block_number", "lane" => lane.to_string()).set(block as f64);
}

pub fn set_lane_mmap_mode(lane: &str, mmap: bool) {
    gauge!("pir_lane_mmap_mode", "lane" => lane.to_string()).set(if mmap { 1.0 } else { 0.0 });
}

pub fn record_reload(lane: &str, status: &str, duration: Duration) {
    counter!("pir_reload_total", "lane" => lane.to_string(), "status" => status.to_string()).increment(1);
    histogram!("pir_reload_duration_seconds", "lane" => lane.to_string(), "status" => status.to_string())
        .record(duration.as_secs_f64());
}

pub fn set_reload_in_progress(lane: &str, in_progress: bool) {
    gauge!("pir_reload_in_progress", "lane" => lane.to_string()).set(if in_progress { 1.0 } else { 0.0 });
}

pub fn set_reload_last_timestamp(lane: &str) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    gauge!("pir_reload_last_timestamp_seconds", "lane" => lane.to_string()).set(ts);
}

pub fn init_prometheus_recorder() -> metrics_exporter_prometheus::PrometheusHandle {
    let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
    builder.install_recorder().expect("Failed to install Prometheus recorder")
}
