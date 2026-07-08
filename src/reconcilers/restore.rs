use std::sync::Arc;

use chrono::Utc;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::{
    api::{Api, Patch, PatchParams, PostParams, ResourceExt},
    Client,
};
use tracing::{debug, error, info, warn};

use crate::adapters::restore_config::build_restore_config_yaml;
use crate::crd::common::{Condition, RestoreInfo, RestoreStatus};
use crate::crd::{KafkaBackup, KafkaRestore, KafkaRestoreStatus};
use crate::error::{Error, Result};
use crate::jobs::job_state::{classify_jobs, JobsState};
use crate::jobs::restore_job::build_restore_job;
use crate::metrics::prometheus::MetricsState;
use crate::reconcilers::{cleanup_delete_params, job_service_account_name, FINALIZER};
use crate::status::conditions::*;
use crate::strimzi::kafka_cr::resolve_kafka_cluster;
use crate::strimzi::kafka_user::resolve_auth;
use crate::strimzi::tls::resolve_cluster_ca;

pub async fn reconcile_restore(
    restore: Arc<KafkaRestore>,
    client: Client,
    _metrics: &MetricsState,
) -> Result<()> {
    let name = restore.name_any();
    let namespace = restore
        .namespace()
        .ok_or(Error::MissingObjectKey(".metadata.namespace"))?;
    let restore_api: Api<KafkaRestore> = Api::namespaced(client.clone(), &namespace);

    // Check if being deleted
    if restore.metadata.deletion_timestamp.is_some() {
        return handle_cleanup(&restore, &client, &namespace).await;
    }

    // Ensure finalizer
    if !restore
        .metadata
        .finalizers
        .as_ref()
        .is_some_and(|f| f.contains(&FINALIZER.to_string()))
    {
        add_finalizer(&restore_api, &name).await?;
    }

    let generation = restore.metadata.generation.unwrap_or(0);

    // Check if restore is already completed — don't re-run
    if let Some(status) = &restore.status {
        if is_condition_true(&status.conditions, CONDITION_TYPE_RESTORE_COMPLETE) {
            debug!(%name, "Restore already completed, skipping");
            return Ok(());
        }
    }

    // Decide from the full set of Jobs for this restore. A Job in any state —
    // running, pending, succeeded, or failed — must suppress creating another
    // one: restores are one-shot, and pod retries belong to the Job's
    // backoffLimit (issue #29: a completed Job has `active=0`, which used to
    // read as "no job running" and re-ran the restore on every requeue).
    let jobs_api: Api<Job> = Api::namespaced(client.clone(), &namespace);
    let lp = kube::api::ListParams::default().labels(&format!(
        "kafkabackup.com/restore={name},kafkabackup.com/type=restore"
    ));
    let jobs = jobs_api.list(&lp).await?;

    match classify_jobs(&jobs.items) {
        JobsState::Succeeded { job_name } => {
            info!(%job_name, "Restore job completed successfully");
            let job_status = jobs
                .items
                .iter()
                .find(|j| j.metadata.name.as_deref() == Some(job_name.as_str()))
                .and_then(|j| j.status.as_ref());
            let now = Utc::now();
            let restore_info = RestoreInfo {
                start_time: job_status
                    .and_then(|s| s.start_time.as_ref())
                    .map(|t| t.0)
                    .unwrap_or(now),
                completion_time: Some(
                    job_status
                        .and_then(|s| s.completion_time.as_ref())
                        .map(|t| t.0)
                        .unwrap_or(now),
                ),
                status: RestoreStatus::Completed,
                restored_topics: None,
                restored_partitions: None,
                restored_bytes: None,
                point_in_time_target: None,
                actual_point_in_time: None,
            };
            update_status_completed(&restore_api, &name, generation, &restore_info).await?;
            return Ok(());
        }
        JobsState::Failed { job_name } => {
            // Terminal: do not re-create the Job. Patch once — repeating the
            // patch would churn lastTransitionTime and retrigger the watch.
            if !has_condition_reason(
                restore.status.as_ref(),
                CONDITION_TYPE_ERROR,
                REASON_RESTORE_FAILED,
            ) {
                error!(%job_name, "Restore job failed");
                update_status_failed(&restore_api, &name, generation, &job_name).await?;
            }
            return Ok(());
        }
        JobsState::InProgress => {
            if !has_condition_reason(
                restore.status.as_ref(),
                CONDITION_TYPE_READY,
                REASON_RESTORE_RUNNING,
            ) {
                update_status_running(&restore_api, &name, generation).await?;
            }
            return Ok(());
        }
        JobsState::NoJobs => {}
    }

    // No Job exists yet — resolve dependencies and create one.

    // Step 1: Resolve source backup CR
    let backup_api: Api<KafkaBackup> = Api::namespaced(client.clone(), &namespace);
    let source_backup = backup_api
        .get(&restore.spec.backup_ref.name)
        .await
        .map_err(|_| Error::BackupNotFound {
            name: restore.spec.backup_ref.name.clone(),
        })?;

    // Step 2: Resolve target Strimzi Kafka cluster
    let kafka_cluster = match resolve_kafka_cluster(
        &client,
        &restore.spec.strimzi_cluster_ref,
        &namespace,
        restore.spec.authentication.as_ref().map(|a| &a.auth_type),
    )
    .await
    {
        Ok(cluster) => cluster,
        Err(e) => {
            update_status_error(&restore_api, &name, generation, &e).await?;
            return Err(e);
        }
    };

    // Step 3: Resolve TLS certificates
    let tls_certs = match resolve_cluster_ca(
        &client,
        &kafka_cluster.name,
        restore.spec.strimzi_cluster_ref.ca_secret.as_ref(),
        &namespace,
    )
    .await
    {
        Ok(certs) => Some(certs),
        Err(e) => {
            warn!(%name, error = %e, "Failed to resolve TLS certs for target cluster");
            None
        }
    };

    // Step 4: Resolve authentication
    let resolved_auth =
        resolve_auth(&client, restore.spec.authentication.as_ref(), &namespace).await?;

    // Step 5: Build restore config YAML and create ConfigMap
    let config_yaml = build_restore_config_yaml(
        &restore,
        &source_backup,
        &kafka_cluster,
        &tls_certs,
        &resolved_auth,
    )?;
    let config_map_name = format!("{name}-config");
    create_or_update_config_map(
        &client,
        &namespace,
        &config_map_name,
        &config_yaml,
        &restore,
    )
    .await?;

    // Step 6: Create the restore Job
    let job_name = format!("{name}-{}", Utc::now().format("%Y%m%d-%H%M%S"));
    let job_service_account = job_service_account_name();
    let job = build_restore_job(
        &restore,
        &job_name,
        &config_map_name,
        &kafka_cluster,
        &resolved_auth,
        &source_backup,
        job_service_account.as_deref(),
    )?;

    jobs_api
        .create(&PostParams::default(), &job)
        .await
        .map_err(|e| Error::JobCreationFailed(e.to_string()))?;

    info!(%job_name, "Created restore job");
    update_status_running(&restore_api, &name, generation).await?;

    Ok(())
}

/// Whether the current status has a condition of `condition_type` with the
/// given reason. Used to avoid re-patching an identical status, which would
/// churn `lastTransitionTime` and retrigger the watch.
fn has_condition_reason(
    status: Option<&KafkaRestoreStatus>,
    condition_type: &str,
    reason: &str,
) -> bool {
    let conditions: &[Condition] = status.map(|s| s.conditions.as_slice()).unwrap_or(&[]);
    find_condition(conditions, condition_type).is_some_and(|c| c.reason.as_deref() == Some(reason))
}

async fn handle_cleanup(restore: &KafkaRestore, client: &Client, namespace: &str) -> Result<()> {
    let name = restore.name_any();
    info!(%name, "Cleaning up KafkaRestore resources");

    // Delete associated jobs
    let jobs_api: Api<Job> = Api::namespaced(client.clone(), namespace);
    let lp = kube::api::ListParams::default().labels(&format!(
        "app.kubernetes.io/managed-by=kafka-backup-operator,kafkabackup.com/restore={name}"
    ));
    if let Ok(job_list) = jobs_api.list(&lp).await {
        for job in job_list {
            let job_name = job.metadata.name.unwrap_or_default();
            let _ = jobs_api.delete(&job_name, &cleanup_delete_params()).await;
        }
    }

    // Delete ConfigMap
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let cm_name = format!("{name}-config");
    let _ = cm_api.delete(&cm_name, &cleanup_delete_params()).await;

    // Remove finalizer
    let restore_api: Api<KafkaRestore> = Api::namespaced(client.clone(), namespace);
    remove_finalizer(&restore_api, &name).await?;

    info!(%name, "Cleanup complete");
    Ok(())
}

async fn add_finalizer(api: &Api<KafkaRestore>, name: &str) -> Result<()> {
    let patch = serde_json::json!({
        "metadata": {
            "finalizers": [FINALIZER]
        }
    });
    api.patch(
        name,
        &PatchParams::apply("kafka-backup-operator"),
        &Patch::Merge(&patch),
    )
    .await?;
    Ok(())
}

async fn remove_finalizer(api: &Api<KafkaRestore>, name: &str) -> Result<()> {
    let patch = serde_json::json!({
        "metadata": {
            "finalizers": null
        }
    });
    api.patch(
        name,
        &PatchParams::apply("kafka-backup-operator"),
        &Patch::Merge(&patch),
    )
    .await?;
    Ok(())
}

async fn create_or_update_config_map(
    client: &Client,
    namespace: &str,
    name: &str,
    config_yaml: &str,
    owner: &KafkaRestore,
) -> Result<()> {
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);

    let cm = serde_json::json!({
        "apiVersion": "v1",
        "kind": "ConfigMap",
        "metadata": {
            "name": name,
            "namespace": namespace,
            "labels": {
                "app.kubernetes.io/managed-by": "kafka-backup-operator",
                "app.kubernetes.io/part-of": "kafka-backup",
                "kafkabackup.com/restore": owner.name_any()
            },
            "ownerReferences": [{
                "apiVersion": "kafkabackup.com/v1alpha1",
                "kind": "KafkaRestore",
                "name": owner.name_any(),
                "uid": owner.metadata.uid.as_deref().unwrap_or(""),
                "controller": true,
                "blockOwnerDeletion": true
            }]
        },
        "data": {
            "restore.yaml": config_yaml
        }
    });

    cm_api
        .patch(
            name,
            &PatchParams::apply("kafka-backup-operator"),
            &Patch::Apply(cm),
        )
        .await?;
    Ok(())
}

async fn update_status_running(api: &Api<KafkaRestore>, name: &str, generation: i64) -> Result<()> {
    let status = KafkaRestoreStatus {
        conditions: vec![not_ready(REASON_RESTORE_RUNNING, "Restore job is running")],
        observed_generation: Some(generation),
        ..Default::default()
    };
    patch_status(api, name, &status).await
}

async fn update_status_completed(
    api: &Api<KafkaRestore>,
    name: &str,
    generation: i64,
    info: &RestoreInfo,
) -> Result<()> {
    let status = KafkaRestoreStatus {
        conditions: vec![
            ready(REASON_RESTORE_COMPLETED, "Restore completed successfully"),
            new_condition(
                CONDITION_TYPE_RESTORE_COMPLETE,
                STATUS_TRUE,
                REASON_RESTORE_COMPLETED,
                "Restore completed successfully",
            ),
        ],
        restore: Some(info.clone()),
        observed_generation: Some(generation),
    };
    patch_status(api, name, &status).await
}

async fn update_status_failed(
    api: &Api<KafkaRestore>,
    name: &str,
    generation: i64,
    job_name: &str,
) -> Result<()> {
    let status = KafkaRestoreStatus {
        conditions: error_conditions(
            REASON_RESTORE_FAILED,
            &format!("Restore job {job_name} failed"),
        ),
        observed_generation: Some(generation),
        ..Default::default()
    };
    patch_status(api, name, &status).await
}

async fn update_status_error(
    api: &Api<KafkaRestore>,
    name: &str,
    generation: i64,
    error: &Error,
) -> Result<()> {
    let status = KafkaRestoreStatus {
        conditions: error_conditions(error.reason(), &error.to_string()),
        observed_generation: Some(generation),
        ..Default::default()
    };
    patch_status(api, name, &status).await
}

async fn patch_status(
    api: &Api<KafkaRestore>,
    name: &str,
    status: &KafkaRestoreStatus,
) -> Result<()> {
    let patch = serde_json::json!({ "status": status });
    api.patch_status(
        name,
        &PatchParams::apply("kafka-backup-operator"),
        &Patch::Merge(&patch),
    )
    .await?;
    Ok(())
}
