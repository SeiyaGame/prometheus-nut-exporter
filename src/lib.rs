//! Prometheus/OpenMetrics exporter for Network UPS Tools (NUT).

pub mod auth;
pub mod config;
pub mod metrics;
pub mod nut;
pub mod openmetrics;
pub mod server;

pub const APP_NAME: &str = "Prometheus NUT Exporter";
pub const APP_AUTHOR: &str = "HON95";
/// Set at build time through the APP_VERSION environment variable (CI and Dockerfile)
pub const APP_VERSION: &str = match option_env!("APP_VERSION") {
    Some(version) => version,
    None => "0.0.0-SNAPSHOT",
};
