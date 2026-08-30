use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use k8s_openapi::api::batch::v1::{CronJob, Job};
use kube::{
    runtime::{
        controller::{Action, Controller},
        watcher::Config,
    },
    Api, Client, ResourceExt,
};
use tokio::time::Duration;
use tracing::{error, info, instrument};

use crate::controllers::{startup_resync_ticks, RunOptions, OWNED_BACKUP_SELECTOR};
use crate::crd::KafkaBackup;
use crate::engine::EngineImageConfig;
use crate::metrics::prometheus::MetricsState;
use crate::reconcilers::backup::reconcile_backup;
use crate::shutdown::Shutdown;

struct Context {
    client: Client,
    metrics: Arc<MetricsState>,
    engine: Arc<EngineImageConfig>,
    reconcile_timeout: Duration,
}

#[instrument(skip(ctx))]
async fn reconcile(
    backup: Arc<KafkaBackup>,
    ctx: Arc<Context>,
) -> Result<Action, crate::error::Error> {
    let name = backup.name_any();
    let namespace = backup.namespace().unwrap_or_default();
    info!(%name, %namespace, "Reconciling KafkaBackup");

    let started = Instant::now();
    let result = match tokio::time::timeout(
        ctx.reconcile_timeout,
        reconcile_backup(backup, ctx.client.clone(), &ctx.metrics, &ctx.engine),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(crate::error::Error::ReconcileTimeout(ctx.reconcile_timeout)),
    };
    ctx.metrics
        .record_reconciliation("backup", result.is_ok(), started.elapsed());
    result?;

    Ok(Action::requeue(Duration::from_secs(300)))
}

fn error_policy(
    backup: Arc<KafkaBackup>,
    error: &crate::error::Error,
    _ctx: Arc<Context>,
) -> Action {
    let name = backup.name_any();
    error!(%name, %error, "Reconciliation error for KafkaBackup");
    Action::requeue(Duration::from_secs(30))
}

pub async fn run(
    client: Client,
    metrics: Arc<MetricsState>,
    engine: Arc<EngineImageConfig>,
    shutdown: Shutdown,
) {
    run_with(client, metrics, engine, shutdown, RunOptions::default()).await
}

pub async fn run_with(
    client: Client,
    metrics: Arc<MetricsState>,
    engine: Arc<EngineImageConfig>,
    shutdown: Shutdown,
    options: RunOptions,
) {
    let backups = Api::<KafkaBackup>::all(client.clone());
    let jobs = Api::<Job>::all(client.clone());
    let cronjobs = Api::<CronJob>::all(client.clone());

    let context = Arc::new(Context {
        client: client.clone(),
        metrics,
        engine,
        reconcile_timeout: options.reconcile_timeout,
    });

    info!("Starting KafkaBackup controller");

    Controller::new(backups, Config::default().any_semantic())
        // Watch owned Jobs so completion/failure updates the KafkaBackup
        // status immediately instead of waiting for the periodic requeue.
        .owns(jobs, Config::default().labels(OWNED_BACKUP_SELECTOR))
        // Watch owned CronJobs so an out-of-band change to the scheduled
        // backup — a manual edit, or the last apply of an operator pod that
        // was still draining during an upgrade — is reverted immediately
        // instead of on the next periodic requeue (issue #62).
        .owns(cronjobs, Config::default().labels(OWNED_BACKUP_SELECTOR))
        .reconcile_all_on(startup_resync_ticks(options.startup_resync_delays))
        .graceful_shutdown_on(shutdown)
        .run(reconcile, error_policy, context)
        .for_each(|res| async move {
            match res {
                Ok(o) => info!("Reconciled KafkaBackup: {:?}", o),
                Err(e) => error!("Reconcile failed: {:?}", e),
            }
        })
        .await;

    info!("KafkaBackup controller shut down");
}
