use std::io::BufRead;
use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use prometheus_nut_exporter::auth::{self, Auth};
use prometheus_nut_exporter::config::{self, Config};
use prometheus_nut_exporter::{metrics, server};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

#[tokio::main]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(config::DEFAULT_LOG_LEVEL)).init();

    match std::env::args().nth(1).as_deref() {
        None => {}
        Some("hash-password") => return hash_password(),
        Some(other) => {
            eprintln!("Unknown command {other:?}. Usage: prometheus-nut-exporter [hash-password]");
            return ExitCode::FAILURE;
        }
    }

    let config = Config::from_env();
    if config.print_metrics_and_exit {
        print!("{}", metrics::markdown_table());
        return ExitCode::SUCCESS;
    }

    let auth = match &config.web_config_file {
        None => None,
        Some(path) => match Auth::load(path) {
            Ok(Some(auth)) => {
                log::info!("Basic authentication enabled ({} users).", auth.user_count());
                Some(Arc::new(auth))
            }
            Ok(None) => {
                log::warn!("{}: no basic_auth_users, the metrics are not protected.", path.display());
                None
            }
            Err(err) => {
                log::error!("Invalid web config file {err}");
                return ExitCode::FAILURE;
            }
        },
    };

    let endpoint = SocketAddr::new(config.http_address, config.http_port);
    let listener = match TcpListener::bind(endpoint).await {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("Failed to listen on http://{endpoint}: {err}");
            return ExitCode::FAILURE;
        }
    };
    log::info!("Listening on http://{endpoint}{}", config.http_path);

    match server::serve(listener, config, auth, shutdown_signal()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log::error!("Server error: {err}");
            ExitCode::FAILURE
        }
    }
}

/// Reads a password on stdin and prints its bcrypt hash, for `basic_auth_users`.
fn hash_password() -> ExitCode {
    eprintln!("Password (read from stdin):");
    let mut password = String::new();
    if std::io::stdin().lock().read_line(&mut password).is_err() {
        return ExitCode::FAILURE;
    }
    let password = password.trim_end_matches(['\r', '\n']);
    if password.is_empty() {
        eprintln!("Empty password.");
        return ExitCode::FAILURE;
    }
    match auth::hash_password(password) {
        Ok(hash) => {
            println!("{hash}");
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("{err}");
            ExitCode::FAILURE
        }
    }
}

/// Resolves on SIGINT or SIGTERM (`docker stop`).
async fn shutdown_signal() {
    let mut sigterm = signal(SignalKind::terminate()).expect("SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => log::debug!("Received interrupt signal."),
        _ = sigterm.recv() => log::debug!("Received termination signal."),
    }
}
