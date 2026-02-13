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

use crate::crd::KafkaRestore;
use crate::metrics::prometheus::MetricsState;
use crate::reconcilers::restore::reconcile_restore;

struct Context {
    client: Client,
    metrics: Arc<MetricsState>,
}

#[instrument(skip(ctx))]
async fn reconcile(
    restore: Arc<KafkaRestore>,
    ctx: Arc<Context>,
) -> Result<Action, crate::error::Error> {
    let name = restore.name_any();
    let namespace = restore.namespace().unwrap_or_default();
    info!(%name, %namespace, "Reconciling KafkaRestore");

    reconcile_restore(restore, ctx.client.clone(), &ctx.metrics).await?;

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

pub async fn run(client: Client, metrics: Arc<MetricsState>) {
    let restores = Api::<KafkaRestore>::all(client.clone());

    let context = Arc::new(Context {
        client: client.clone(),
        metrics,
    });

    info!("Starting KafkaRestore controller");

    Controller::new(restores, Config::default().any_semantic())
        .shutdown_on_signal()
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
