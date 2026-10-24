//! Basic authentication through the HTTP router.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode, header};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use prometheus_nut_exporter::auth::Auth;
use prometheus_nut_exporter::config::Config;
use prometheus_nut_exporter::server;
use tower::ServiceExt;

async fn get(uri: &str, credentials: Option<(&str, &str)>) -> (StatusCode, Option<String>) {
    // Lowest bcrypt cost: the tests stay fast
    let hash = bcrypt::hash("secret", 4).unwrap();
    let auth = Auth::from_yaml(&format!("basic_auth_users:\n  scraper: {hash}\n")).unwrap().unwrap();
    let app = server::router(Config::default(), Some(Arc::new(auth)))
        .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))));

    let mut request = Request::get(uri);
    if let Some((user, password)) = credentials {
        request =
            request.header(header::AUTHORIZATION, format!("Basic {}", BASE64.encode(format!("{user}:{password}"))));
    }
    let response = app.oneshot(request.body(Body::empty()).unwrap()).await.unwrap();
    let challenge = response.headers().get(header::WWW_AUTHENTICATE).map(|v| v.to_str().unwrap().to_owned());
    (response.status(), challenge)
}

#[tokio::test]
async fn metrics_need_credentials() {
    assert_eq!(get("/nut?target=127.0.0.1:1", None).await, (StatusCode::UNAUTHORIZED, Some("Basic".into())));
    assert_eq!(get("/nut?target=127.0.0.1:1", Some(("scraper", "wrong"))).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(get("/nut?target=127.0.0.1:1", Some(("nobody", "secret"))).await.0, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn valid_credentials_reach_the_scrape() {
    // Authenticated: the scrape runs, and fails as nothing listens on port 1
    assert_eq!(get("/nut?target=127.0.0.1:1", Some(("scraper", "secret"))).await.0, StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn home_stays_open_for_the_health_check() {
    assert_eq!(get("/", None).await.0, StatusCode::OK);
}
