//! End-to-end: a fake NUT server serves the fixtures, the exporter scrapes it through HTTP.
//!
//! `UPDATE_SNAPSHOTS=1 cargo test` rewrites `tests/fixtures/expected.txt` after an intended change.

use std::net::SocketAddr;
use std::path::Path;

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use prometheus_nut_exporter::config::Config;
use prometheus_nut_exporter::{APP_VERSION, server};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tower::ServiceExt;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(FIXTURES).join(name)).unwrap().replace("\r\n", "\n")
}

/// Starts a NUT server answering from the fixtures and returns its address.
async fn fake_nut_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let (reader, mut writer) = stream.into_split();
                let mut lines = BufReader::new(reader).lines();
                while let Ok(Some(query)) = lines.next_line().await {
                    let answer = match query.split(' ').collect::<Vec<_>>().as_slice() {
                        ["VER"] => fixture("VER-1.txt"),
                        ["LIST", "UPS"] => fixture("LIST-UPS.txt"),
                        ["LIST", "VAR", ups] => fixture(&format!("LIST-VAR-{}.txt", ups.trim_start_matches("ups-"))),
                        ["LOGOUT"] => "OK Goodbye\n".to_owned(),
                        _ => "ERR UNKNOWN-COMMAND\n".to_owned(),
                    };
                    writer.write_all(answer.as_bytes()).await.unwrap();
                }
            });
        }
    });
    address
}

async fn get(uri: &str, accept: &str) -> (StatusCode, String, String) {
    let request = Request::get(uri).header(header::ACCEPT, accept).body(Body::empty()).unwrap();
    let app = server::router(Config::default(), None).layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))));
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let content_type = response.headers().get(header::CONTENT_TYPE).map_or("", |v| v.to_str().unwrap()).to_owned();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, content_type, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn scrape_matches_snapshot() {
    let nut = fake_nut_server().await;
    let (status, content_type, body) = get(&format!("/nut?target={nut}"), "application/openmetrics-text").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(content_type, "application/openmetrics-text; version=1.0.0; charset=UTF-8");

    // The exporter version depends on the build
    let body = body
        .replace(&format!("nut_exporter_info{{version=\"{APP_VERSION}\"}}"), "nut_exporter_info{version=\"VERSION\"}");
    let snapshot = Path::new(FIXTURES).join("expected.txt");
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() {
        std::fs::write(&snapshot, &body).unwrap();
    }
    assert_eq!(body, fixture("expected.txt"), "output changed, UPDATE_SNAPSHOTS=1 cargo test to accept it");
}

#[tokio::test]
async fn plain_text_without_openmetrics_accept() {
    let nut = fake_nut_server().await;
    let (status, content_type, _) = get(&format!("/nut?target={nut}"), "text/plain").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "text/plain; charset=UTF-8");
}

#[tokio::test]
async fn errors() {
    let (status, _, body) = get("/nut", "*/*").await;
    assert_eq!((status, body.lines().next().unwrap()), (StatusCode::BAD_REQUEST, "Missing target."));

    let (status, _, body) = get("/nut?target=a:b:c", "*/*").await;
    assert_eq!((status, body.lines().next().unwrap()), (StatusCode::BAD_REQUEST, "Malformed target."));

    // Nothing listens on port 1
    let (status, _, body) = get("/nut?target=127.0.0.1:1", "*/*").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.starts_with("Failed to connect to target:"), "{body}");

    let (status, _, _) = get("/other", "*/*").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn home_shows_usage() {
    let (status, _, body) = get("/", "*/*").await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.ends_with("Usage: /nut?target=<target>\n"), "{body}");
}
