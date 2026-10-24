//! Configuration from environment variables.

use std::net::{IpAddr, Ipv6Addr};
use std::path::PathBuf;
use std::str::FromStr;

pub const DEFAULT_LOG_LEVEL: &str = "info";
pub const DEFAULT_NUT_PORT: u16 = 3493;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub http_address: IpAddr,
    pub http_port: u16,
    /// Always starts with `/`
    pub http_path: String,
    pub print_metrics_and_exit: bool,
    /// exporter-toolkit web config file (basic authentication)
    pub web_config_file: Option<PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            http_address: IpAddr::V6(Ipv6Addr::UNSPECIFIED),
            http_port: 9995,
            http_path: "/nut".to_owned(),
            print_metrics_and_exit: false,
            web_config_file: None,
        }
    }
}

impl Config {
    pub fn from_env() -> Self {
        Self::from_lookup(|name| std::env::var(name).ok())
    }

    /// Invalid values are logged and replaced with the default.
    fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Self {
        let mut config = Config::default();
        parse_into(&lookup, "HTTP_ADDRESS", &mut config.http_address);
        parse_into(&lookup, "HTTP_PORT", &mut config.http_port);
        parse_into(&lookup, "PRINT_METRICS_AND_EXIT", &mut config.print_metrics_and_exit);
        config.web_config_file = lookup("WEB_CONFIG_FILE").filter(|path| !path.trim().is_empty()).map(PathBuf::from);
        if let Some(path) = lookup("HTTP_PATH") {
            let path = path.trim();
            config.http_path = if path.starts_with('/') { path.to_owned() } else { format!("/{path}") };
        }
        config
    }
}

fn parse_into<T: FromStr>(lookup: &impl Fn(&str) -> Option<String>, name: &str, target: &mut T) {
    let Some(raw) = lookup(name) else { return };
    match raw.trim().parse() {
        Ok(value) => *target = value,
        Err(_) => log::warn!("Ignoring invalid {name}={raw:?}, using the default value."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(vars: &[(&str, &str)]) -> Config {
        Config::from_lookup(|name| vars.iter().find(|(n, _)| *n == name).map(|(_, v)| v.to_string()))
    }

    #[test]
    fn defaults() {
        assert_eq!(config(&[]), Config::default());
    }

    #[test]
    fn values() {
        let config = config(&[
            ("HTTP_ADDRESS", "127.0.0.1"),
            ("HTTP_PORT", "9199"),
            ("HTTP_PATH", "/metrics"),
            ("PRINT_METRICS_AND_EXIT", "true"),
            ("WEB_CONFIG_FILE", "/etc/nut-exporter/web-config.yml"),
        ]);
        assert_eq!(config.http_address, IpAddr::from([127, 0, 0, 1]));
        assert_eq!(config.http_port, 9199);
        assert_eq!(config.http_path, "/metrics");
        assert!(config.print_metrics_and_exit);
        assert_eq!(config.web_config_file, Some(PathBuf::from("/etc/nut-exporter/web-config.yml")));
    }

    #[test]
    fn path_without_slash() {
        assert_eq!(config(&[("HTTP_PATH", "metrics")]).http_path, "/metrics");
    }

    #[test]
    fn invalid_values_keep_the_default() {
        let config = config(&[("HTTP_PORT", "99999"), ("HTTP_ADDRESS", "localhost")]);
        assert_eq!(config, Config::default());
    }
}
