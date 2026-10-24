//! HTTP basic authentication, configured like the official Prometheus exporters (exporter-toolkit):
//! a web config file with `basic_auth_users`, a bcrypt hash per user.
//! https://prometheus.io/docs/prometheus/latest/configuration/https/

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::Deserialize;
use serde::de::IgnoredAny;
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;

/// A bcrypt check takes ~50 ms of CPU: limits the load of a brute force (and the threads used)
const MAX_CONCURRENT_CHECKS: usize = 2;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WebConfig {
    #[serde(default)]
    basic_auth_users: HashMap<String, String>,
    // Options of exporter-toolkit that are refused rather than silently ignored
    tls_server_config: Option<IgnoredAny>,
    http_server_config: Option<IgnoredAny>,
}

pub struct Auth {
    /// User to bcrypt hash
    users: HashMap<String, String>,
    /// Last password that matched the hash of each user, to skip bcrypt on every scrape
    verified: Mutex<HashMap<String, String>>,
    checks: Semaphore,
    /// Checked for unknown users, so that their answer takes as long as for known ones
    dummy_hash: String,
}

impl Auth {
    /// Reads a web config file. `Ok(None)` when it has no users: no authentication.
    pub fn load(path: &Path) -> Result<Option<Auth>, String> {
        let text = std::fs::read_to_string(path).map_err(|err| format!("{}: {err}", path.display()))?;
        Self::from_yaml(&text).map_err(|err| format!("{}: {err}", path.display()))
    }

    pub fn from_yaml(text: &str) -> Result<Option<Auth>, String> {
        if text.trim().is_empty() {
            return Ok(None);
        }
        let config: WebConfig = serde_norway::from_str(text).map_err(|err| err.to_string())?;
        if config.tls_server_config.is_some() {
            return Err("tls_server_config is not supported, use a reverse proxy for TLS".into());
        }
        if config.http_server_config.is_some() {
            return Err("http_server_config is not supported".into());
        }

        // Any cost of the users: the dummy hash takes as long to check as theirs
        let mut cost = None;
        for (user, hash) in &config.basic_auth_users {
            let parts: bcrypt::HashParts =
                hash.parse().map_err(|_| format!("basic_auth_users: the password of {user:?} is not a bcrypt hash"))?;
            cost = Some(parts.get_cost());
        }
        let Some(cost) = cost else { return Ok(None) };

        Ok(Some(Auth {
            users: config.basic_auth_users,
            verified: Mutex::new(HashMap::new()),
            checks: Semaphore::new(MAX_CONCURRENT_CHECKS),
            dummy_hash: bcrypt::hash("", cost).map_err(|err| err.to_string())?,
        }))
    }

    pub fn user_count(&self) -> usize {
        self.users.len()
    }

    async fn check(&self, headers: &HeaderMap) -> bool {
        let Some((user, password)) = credentials(headers) else { return false };
        let hash = self.users.get(&user);

        if hash.is_some() {
            let verified = self.verified.lock().unwrap();
            if verified.get(&user).is_some_and(|known| bool::from(known.as_bytes().ct_eq(password.as_bytes()))) {
                return true;
            }
        }

        let Ok(_permit) = self.checks.acquire().await else { return false };
        let checked_hash = hash.unwrap_or(&self.dummy_hash).clone();
        let checked_password = password.clone();
        let matches = tokio::task::spawn_blocking(move || bcrypt::verify(checked_password, &checked_hash))
            .await
            .is_ok_and(|result| result.unwrap_or(false));

        let valid = matches && hash.is_some();
        if valid {
            self.verified.lock().unwrap().insert(user, password);
        }
        valid
    }
}

/// `Authorization: Basic base64(user:password)`
fn credentials(headers: &HeaderMap) -> Option<(String, String)> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, encoded) = value.split_once(' ')?;
    if !scheme.eq_ignore_ascii_case("basic") {
        return None;
    }
    let decoded = String::from_utf8(BASE64.decode(encoded.trim()).ok()?).ok()?;
    let (user, password) = decoded.split_once(':')?;
    Some((user.to_owned(), password.to_owned()))
}

/// Middleware of the protected routes.
pub async fn require(State(auth): State<Arc<Auth>>, request: Request, next: Next) -> Response {
    if auth.check(request.headers()).await {
        next.run(request).await
    } else {
        (StatusCode::UNAUTHORIZED, [(header::WWW_AUTHENTICATE, "Basic")], "Unauthorized\n").into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic(user: &str, password: &str) -> HeaderMap {
        let value = format!("Basic {}", BASE64.encode(format!("{user}:{password}")));
        HeaderMap::from_iter([(header::AUTHORIZATION, value.parse().unwrap())])
    }

    fn auth_for(user: &str, password: &str) -> Auth {
        // Lowest cost: the tests stay fast
        let hash = bcrypt::hash(password, 4).unwrap();
        Auth::from_yaml(&format!("basic_auth_users:\n  {user}: {hash}\n")).unwrap().unwrap()
    }

    #[tokio::test]
    async fn checks_the_password() {
        let auth = auth_for("scraper", "secret");
        assert!(auth.check(&basic("scraper", "secret")).await);
        // Second time from the cache
        assert!(auth.check(&basic("scraper", "secret")).await);
        assert!(!auth.check(&basic("scraper", "wrong")).await);
        assert!(!auth.check(&basic("other", "secret")).await);
        assert!(!auth.check(&HeaderMap::new()).await);
    }

    #[test]
    fn credentials_formats() {
        assert_eq!(credentials(&basic("a", "b:c")), Some(("a".into(), "b:c".into())));
        let header = |value: &str| HeaderMap::from_iter([(header::AUTHORIZATION, value.parse().unwrap())]);
        assert_eq!(credentials(&header("basic YTpi")), Some(("a".into(), "b".into())));
        assert_eq!(credentials(&header("Bearer YTpi")), None);
        assert_eq!(credentials(&header("Basic not-base64!")), None);
        assert_eq!(credentials(&header("Basic YWI=")), None); // "ab": no colon
    }

    #[test]
    fn config_files() {
        assert!(Auth::from_yaml("").unwrap().is_none());
        assert!(Auth::from_yaml("basic_auth_users: {}\n").unwrap().is_none());
        let err = |text: &str| Auth::from_yaml(text).err().unwrap();
        assert!(err("basic_auth_users:\n  a: plain-password\n").contains("not a bcrypt hash"));
        assert!(err("tls_server_config:\n  cert_file: a.crt\n").contains("reverse proxy"));
        assert!(err("basic_auth_user:\n  a: b\n").contains("unknown field"));
    }

    /// The README example comes from `htpasswd` (`$2y$` hashes) for the password "changeme"
    #[tokio::test]
    async fn readme_example_is_valid() {
        let readme = include_str!("../README.md");
        let example =
            readme.lines().skip_while(|line| !line.starts_with("basic_auth_users:")).take(2).collect::<Vec<_>>();
        let auth = Auth::from_yaml(&example.join("\n")).unwrap().unwrap();
        assert!(auth.users["scraper"].starts_with("$2y$"));
        assert!(auth.check(&basic("scraper", "changeme")).await);
    }
}
