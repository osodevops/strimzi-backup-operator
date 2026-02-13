use std::sync::Arc;

use futures::StreamExt;
use kube::{
    runtime::{
        controller::{Action, Controller},
        watcher::Config,
    },
    Api, Client, ResourceExt,
};
use tokio::time::Duration;
use tracing::{error, info, instrument};

use crate::crd::KafkaBackup;
use crate::metrics::prometheus::MetricsState;
use crate::reconcilers::backup::reconcile_backup;

struct Context {
    client: Client,
    metrics: Arc<MetricsState>,
}

#[instrument(skip(ctx))]
async fn reconcile(
    backup: Arc<KafkaBackup>,
    ctx: Arc<Context>,
) -> Result<Action, crate::error::Error> {
    let name = backup.name_any();
    let namespace = backup.namespace().unwrap_or_default();
    info!(%name, %namespace, "Reconciling KafkaBackup");

    reconcile_backup(backup, ctx.client.clone(), &ctx.metrics).await?;

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

pub async fn run(client: Client, metrics: Arc<MetricsState>) {
    let backups = Api::<KafkaBackup>::all(client.clone());

    let context = Arc::new(Context {
        client: client.clone(),
        metrics,
    });

    info!("Starting KafkaBackup controller");

    Controller::new(backups, Config::default().any_semantic())
        .shutdown_on_signal()
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
