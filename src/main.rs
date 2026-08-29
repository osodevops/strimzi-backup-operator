use std::net::SocketAddr;
use std::sync::Arc;

use futures::future::join;
use kube::Client;
use tokio::sync::watch;
use tokio::task::JoinError;
use tracing::{error, info};

use kafka_backup_operator::controllers::{backup, restore};
use kafka_backup_operator::leader::{
    self, LeaderElectionConfig, LeaderElector, LeaderError, LeaderState,
};
use kafka_backup_operator::metrics::prometheus::MetricsState;
use kafka_backup_operator::{server, shutdown};

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

    let shutdown = shutdown::listen();
    let metrics_state = Arc::new(MetricsState::new());

    let namespace = std::env::var(leader::OPERATOR_NAMESPACE_ENV)
        .ok()
        .map(|ns| ns.trim().to_string())
        .filter(|ns| !ns.is_empty())
        .unwrap_or_else(|| client.default_namespace().to_string());
    let leader_config = LeaderElectionConfig::from_env(&namespace)?;
    let identity = leader::identity();

    // Until this replica has observed the lease it is neither ready nor
    // allowed to reconcile. Without leader election it leads from the start.
    let initial_state = if leader_config.is_some() {
        LeaderState::Unknown
    } else {
        LeaderState::Leader
    };
    let (state_tx, state_rx) = watch::channel(initial_state);

    // Mirror the state into the leader gauge.
    let gauge_task = {
        let metrics = Arc::clone(&metrics_state);
        let mut rx = state_rx.clone();
        let identity = identity.clone();
        tokio::spawn(async move {
            loop {
                let leading = *rx.borrow_and_update() == LeaderState::Leader;
                metrics.set_leader(&identity, leading);
                if rx.changed().await.is_err() {
                    break;
                }
            }
        })
    };

    // Health, readiness and metrics server.
    let health_server = {
        let router = server::router(Arc::clone(&metrics_state), state_rx.clone());
        let shutdown = shutdown.clone();
        tokio::spawn(async move {
            let addr = SocketAddr::from(([0, 0, 0, 0], 9090));
            info!(%addr, "Starting health/metrics server");
            let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
            if let Err(e) = axum::serve(listener, router)
                .with_graceful_shutdown(shutdown)
                .await
            {
                error!(error = %e, "Health/metrics server error");
            }
        })
    };

    match leader_config {
        None => {
            info!("Leader election disabled; starting controllers");
            let controllers = join(
                backup::run(client.clone(), Arc::clone(&metrics_state), shutdown.clone()),
                restore::run(client.clone(), Arc::clone(&metrics_state), shutdown.clone()),
            );
            info!("Controllers started, watching for KafkaBackup and KafkaRestore resources");
            controllers.await;
        }
        Some(config) => {
            info!(
                %identity,
                lease = %config.lease_name,
                namespace = %config.namespace,
                lease_duration = ?config.lease_duration,
                renew_deadline = ?config.renew_deadline,
                retry_period = ?config.retry_period,
                "Leader election enabled; waiting for the leader lease"
            );
            let (release_tx, release_rx) = watch::channel(false);
            let elector = LeaderElector::new(client.clone(), config, identity, state_tx);
            let mut elector_task = tokio::spawn(elector.run(release_rx));

            // Standby: block until we lead, we are told to stop, or the elector dies.
            let mut wait_rx = state_rx.clone();
            tokio::select! {
                waited = wait_rx.wait_for(|s| *s == LeaderState::Leader) => {
                    // Err means the elector dropped its sender, i.e. it exited
                    // before we ever led: never start the controllers then.
                    if waited.is_err() {
                        fatal_exit(elector_task.await);
                    }
                }
                _ = shutdown.clone() => {
                    info!("Shutdown requested while standing by");
                    let _ = release_tx.send(true);
                    let _ = elector_task.await;
                    let _ = health_server.await;
                    return Ok(());
                }
                outcome = &mut elector_task => fatal_exit(outcome),
            }
            info!("Acquired leadership; starting controllers");

            let controllers = join(
                backup::run(client.clone(), Arc::clone(&metrics_state), shutdown.clone()),
                restore::run(client.clone(), Arc::clone(&metrics_state), shutdown.clone()),
            );
            info!("Controllers started, watching for KafkaBackup and KafkaRestore resources");

            tokio::select! {
                _ = controllers => {
                    // Shutdown path: the controllers have drained their in-flight
                    // reconciles. Only now hand the lease over, so our successor
                    // cannot start reconciling while we may still be writing
                    // (issue #62).
                    info!("Controllers drained; releasing the leader lease");
                    let _ = release_tx.send(true);
                    let _ = elector_task.await;
                }
                outcome = &mut elector_task => fatal_exit(outcome),
            }
        }
    }

    drop(state_rx);
    let _ = gauge_task.await;
    let _ = health_server.await;
    info!("Operator shutting down");
    Ok(())
}

/// The elector stopped on its own: leadership was lost or the lease could not
/// be renewed in time. In-flight reconciles cannot be cancelled cleanly, so
/// the only safe reaction is to exit and let the kubelet restart the process,
/// which then rejoins the election as a candidate.
fn fatal_exit(outcome: Result<Result<(), LeaderError>, JoinError>) -> ! {
    match outcome {
        Ok(Ok(())) => error!("Leader elector stopped unexpectedly; exiting"),
        Ok(Err(e)) => {
            error!(error = %e, "Leader election failed; exiting so the pod restarts as a candidate")
        }
        Err(e) => error!(error = %e, "Leader elector task panicked; exiting"),
    }
    std::process::exit(1);
}
