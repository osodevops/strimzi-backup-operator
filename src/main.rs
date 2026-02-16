use std::net::SocketAddr;
use std::sync::Arc;

use axum::{routing::get, Router};
use futures::future::join3;
use kube::Client;
use tracing::{error, info};

use kafka_backup_operator::controllers::{backup, restore};
use kafka_backup_operator::metrics::prometheus::MetricsState;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,kube=info".into()),
        )
        .json()
        .init();

    info!(
        version = env!("CARGO_PKG_VERSION"),
        "Starting kafka-backup-operator"
    );

    let client = Client::try_default().await?;
    info!("Connected to Kubernetes API server");

    let metrics_state = Arc::new(MetricsState::new());

    // Health and metrics server
    let health_metrics_server = {
        let metrics_state = Arc::clone(&metrics_state);
        async move {
            let app = Router::new()
                .route("/healthz", get(|| async { "ok" }))
                .route("/readyz", get(|| async { "ok" }))
                .route(
                    "/metrics",
                    get(move || {
                        let state = Arc::clone(&metrics_state);
                        async move { state.gather() }
                    }),
                );

            let addr = SocketAddr::from(([0, 0, 0, 0], 9090));
            info!(%addr, "Starting health/metrics server");
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            if let Err(e) = axum::serve(listener, app)
                .with_graceful_shutdown(shutdown_signal())
                .await
            {
                error!(error = %e, "Health/metrics server error");
            }
        }
    };

    // Run both controllers and the health server concurrently
    let backup_controller = backup::run(client.clone(), Arc::clone(&metrics_state));
    let restore_controller = restore::run(client.clone(), Arc::clone(&metrics_state));

    info!("Controllers started, watching for KafkaBackup and KafkaRestore resources");

    join3(backup_controller, restore_controller, health_metrics_server).await;

    info!("Operator shutting down");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => info!("Received Ctrl+C, shutting down"),
        _ = terminate => info!("Received SIGTERM, shutting down"),
    }
}
