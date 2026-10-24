use std::net::SocketAddr;
use std::process::ExitCode;

use prometheus_nut_exporter::config::{self, Config};
use prometheus_nut_exporter::{metrics, server};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

#[tokio::main]
async fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(config::DEFAULT_LOG_LEVEL)).init();

    let config = Config::from_env();
    if config.print_metrics_and_exit {
        print!("{}", metrics::markdown_table());
        return ExitCode::SUCCESS;
    }

    let endpoint = SocketAddr::new(config.http_address, config.http_port);
    let listener = match TcpListener::bind(endpoint).await {
        Ok(listener) => listener,
        Err(err) => {
            log::error!("Failed to listen on http://{endpoint}: {err}");
            return ExitCode::FAILURE;
        }
    };
    log::info!("Listening on http://{endpoint}{}", config.http_path);

    match server::serve(listener, config, shutdown_signal()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log::error!("Server error: {err}");
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
