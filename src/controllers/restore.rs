use std::sync::Arc;
use std::time::Instant;

use futures::StreamExt;
use k8s_openapi::api::batch::v1::Job;
use kube::{
    runtime::{
        controller::{Action, Controller},
        watcher::Config,
    },
    Api, Client, ResourceExt,
};
use tokio::time::Duration;
use tracing::{error, info, instrument};

use crate::controllers::{startup_resync_ticks, RunOptions, OWNED_RESTORE_SELECTOR};
use crate::crd::KafkaRestore;
use crate::engine::EngineImageConfig;
use crate::metrics::prometheus::MetricsState;
use crate::reconcilers::restore::reconcile_restore;
use crate::shutdown::Shutdown;

struct Context {
    client: Client,
    metrics: Arc<MetricsState>,
    engine: Arc<EngineImageConfig>,
    reconcile_timeout: Duration,
}

#[instrument(skip(ctx))]
async fn reconcile(
    restore: Arc<KafkaRestore>,
    ctx: Arc<Context>,
) -> Result<Action, crate::error::Error> {
    let name = restore.name_any();
    let namespace = restore.namespace().unwrap_or_default();
    info!(%name, %namespace, "Reconciling KafkaRestore");

    let started = Instant::now();
    let result = match tokio::time::timeout(
        ctx.reconcile_timeout,
        reconcile_restore(restore, ctx.client.clone(), &ctx.metrics, &ctx.engine),
    )
    .await
    {
        Ok(result) => result,
        Err(_) => Err(crate::error::Error::ReconcileTimeout(ctx.reconcile_timeout)),
    };
    ctx.metrics
        .record_reconciliation("restore", result.is_ok(), started.elapsed());
    result?;

    Ok(Action::requeue(Duration::from_secs(300)))
}

fn error_policy(
    restore: Arc<KafkaRestore>,
    error: &crate::error::Error,
    _ctx: Arc<Context>,
) -> Action {
    let name = restore.name_any();
    error!(%name, %error, "Reconciliation error for KafkaRestore");
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
    let restores = Api::<KafkaRestore>::all(client.clone());
    let jobs = Api::<Job>::all(client.clone());

    let context = Arc::new(Context {
        client: client.clone(),
        metrics,
        engine,
        reconcile_timeout: options.reconcile_timeout,
    });

    info!("Starting KafkaRestore controller");

    Controller::new(restores, Config::default().any_semantic())
        // Watch owned Jobs so completion/failure updates the KafkaRestore
        // status immediately instead of waiting for the periodic requeue.
        .owns(jobs, Config::default().labels(OWNED_RESTORE_SELECTOR))
        .reconcile_all_on(startup_resync_ticks(options.startup_resync_delays))
        .graceful_shutdown_on(shutdown)
        .run(reconcile, error_policy, context)
        .for_each(|res| async move {
            match res {
                Ok(o) => info!("Reconciled KafkaRestore: {:?}", o),
                Err(e) => error!("Reconcile failed: {:?}", e),
            }
        })
        .await;

    info!("KafkaRestore controller shut down");
}
