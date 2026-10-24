//! HTTP server: `/` shows the usage, the metrics path scrapes `?target=<host>[:port]`.

use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::extract::{ConnectInfo, Query, Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use tokio::net::TcpListener;

use crate::config::{Config, DEFAULT_NUT_PORT};
use crate::{APP_AUTHOR, APP_NAME, APP_VERSION, nut, openmetrics};

const CONTENT_TYPE_TEXT: &str = "text/plain; charset=UTF-8";
const CONTENT_TYPE_OPENMETRICS: &str = "application/openmetrics-text; version=1.0.0; charset=UTF-8";

/// Used when Prometheus does not send its scrape timeout
const DEFAULT_SCRAPE_TIMEOUT: Duration = Duration::from_secs(10);
/// Kept from the Prometheus scrape timeout to send the answer in time
const SCRAPE_TIMEOUT_MARGIN: Duration = Duration::from_millis(500);

pub async fn serve(
    listener: TcpListener,
    config: Config,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    let app = router(config).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app).with_graceful_shutdown(shutdown).await
}

pub fn router(config: Config) -> Router {
    let config = Arc::new(config);
    let mut router = Router::new().route("/", get(home));
    if config.http_path != "/" {
        router = router.route(&config.http_path, get(metrics));
    }
    router
        .fallback(|| async { (StatusCode::NOT_FOUND, "Not found\n") })
        .method_not_allowed_fallback(|| async { (StatusCode::METHOD_NOT_ALLOWED, "Method not allowed\n") })
        .layer(middleware::from_fn(log_request))
        .with_state(config)
}

async fn log_request(ConnectInfo(remote): ConnectInfo<SocketAddr>, request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let response = next.run(request).await;
    log::debug!("Request: {remote} {method} {path} {}", response.status());
    response
}

async fn home(State(config): State<Arc<Config>>) -> String {
    format!("{APP_NAME} version {APP_VERSION} by {APP_AUTHOR}.\n\nUsage: {}?target=<target>\n", config.http_path)
}

async fn metrics(
    State(config): State<Arc<Config>>,
    Query(query): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let usage = format!("Usage: {}?target=<target>", config.http_path);
    let target = match query.get("target").map(|raw| parse_target(raw)) {
        Some(Some(target)) => target,
        Some(None) => return (StatusCode::BAD_REQUEST, format!("Malformed target.\n\n{usage}")).into_response(),
        None => return (StatusCode::BAD_REQUEST, format!("Missing target.\n\n{usage}")).into_response(),
    };

    let scrape = match nut::scrape_target(&target, scrape_timeout(&headers)).await {
        Ok(scrape) => scrape,
        Err(err) => {
            log::debug!("Scrape of {target} failed: {err}");
            return (StatusCode::SERVICE_UNAVAILABLE, err.to_string()).into_response();
        }
    };

    let accepts_openmetrics = headers
        .get(header::ACCEPT)
        .and_then(|accept| accept.to_str().ok())
        .is_some_and(|accept| accept.contains("application/openmetrics-text"));
    let content_type = if accepts_openmetrics { CONTENT_TYPE_OPENMETRICS } else { CONTENT_TYPE_TEXT };
    ([(header::CONTENT_TYPE, content_type)], openmetrics::render(&scrape, APP_VERSION)).into_response()
}

fn scrape_timeout(headers: &HeaderMap) -> Duration {
    headers
        .get("x-prometheus-scrape-timeout-seconds")
        .and_then(|value| value.to_str().ok()?.parse::<f64>().ok())
        .and_then(|seconds| Duration::try_from_secs_f64(seconds).ok())
        .map_or(DEFAULT_SCRAPE_TIMEOUT, |timeout| {
            timeout.saturating_sub(SCRAPE_TIMEOUT_MARGIN).max(SCRAPE_TIMEOUT_MARGIN)
        })
}

/// `host`, `host:port`, `[ipv6]` or `[ipv6]:port` to `host:port`.
fn parse_target(raw: &str) -> Option<String> {
    let (host, port) = match raw.strip_prefix('[') {
        Some(rest) => {
            let (address, after) = rest.split_once(']')?;
            if address.is_empty() {
                return None;
            }
            let port = if after.is_empty() { None } else { Some(after.strip_prefix(':')?) };
            (format!("[{address}]"), port)
        }
        None => match raw.split_once(':') {
            Some((host, port)) => (host.to_owned(), Some(port)),
            None => (raw.to_owned(), None),
        },
    };
    if host.is_empty() {
        return None;
    }
    let port = match port {
        Some(port) => port.parse::<u16>().ok()?,
        None => DEFAULT_NUT_PORT,
    };
    Some(format!("{host}:{port}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets() {
        assert_eq!(parse_target("nut.lan").unwrap(), "nut.lan:3493");
        assert_eq!(parse_target("10.0.0.2:3494").unwrap(), "10.0.0.2:3494");
        assert_eq!(parse_target("[::1]").unwrap(), "[::1]:3493");
        assert_eq!(parse_target("[fd00::2]:3494").unwrap(), "[fd00::2]:3494");
        for bad in ["", ":3493", "host:", "host:port", "a:1:2", "::1", "[]", "[::1", "[::1]3493", "host:99999"] {
            assert_eq!(parse_target(bad), None, "{bad}");
        }
    }

    #[test]
    fn timeouts() {
        let headers = |value: &str| {
            HeaderMap::from_iter([("x-prometheus-scrape-timeout-seconds".parse().unwrap(), value.parse().unwrap())])
        };
        assert_eq!(scrape_timeout(&HeaderMap::new()), DEFAULT_SCRAPE_TIMEOUT);
        assert_eq!(scrape_timeout(&headers("5")), Duration::from_millis(4500));
        assert_eq!(scrape_timeout(&headers("0.1")), SCRAPE_TIMEOUT_MARGIN);
        assert_eq!(scrape_timeout(&headers("-1")), DEFAULT_SCRAPE_TIMEOUT);
    }
}
