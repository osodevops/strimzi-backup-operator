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
use crate::crd::common::{RestoreInfo, RestoreStatus};
use crate::crd::{KafkaBackup, KafkaRestore, KafkaRestoreStatus};
use crate::error::{Error, Result};
use crate::jobs::restore_job::build_restore_job;
use crate::metrics::prometheus::MetricsState;
use crate::reconcilers::FINALIZER;
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

    // Step 1: Resolve source backup CR
    let backup_api: Api<KafkaBackup> = Api::namespaced(client.clone(), &namespace);
    let source_backup = backup_api
        .get(&restore.spec.backup_ref.name)
        .await
        .map_err(|_| Error::BackupNotFound {
            name: restore.spec.backup_ref.name.clone(),
        })?;

    // Step 2: Resolve target Strimzi Kafka cluster
    let kafka_cluster =
        match resolve_kafka_cluster(&client, &restore.spec.strimzi_cluster_ref, &namespace).await {
            Ok(cluster) => cluster,
            Err(e) => {
                update_status_error(&restore_api, &name, generation, &e).await?;
                return Err(e);
            }
        };

    // Step 3: Resolve TLS certificates
    let tls_certs = match resolve_cluster_ca(&client, &kafka_cluster.name, &namespace).await {
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

    // Step 6: Create restore Job if not already running
    let jobs_api: Api<Job> = Api::namespaced(client.clone(), &namespace);
    if !is_job_running(&jobs_api, &name).await? {
        let job_name = format!("{name}-{}", Utc::now().format("%Y%m%d-%H%M%S"));
        let job = build_restore_job(
            &restore,
            &job_name,
            &config_map_name,
            &kafka_cluster,
            &resolved_auth,
            &source_backup,
        )?;

        jobs_api
            .create(&PostParams::default(), &job)
            .await
            .map_err(|e| Error::JobCreationFailed(e.to_string()))?;

        info!(%job_name, "Created restore job");
        update_status_running(&restore_api, &name, generation).await?;
    }

    // Step 7: Check job completion
    check_job_completion(&client, &restore_api, &restore, generation).await?;

    Ok(())
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
            let _ = jobs_api
                .delete(&job_name, &kube::api::DeleteParams::default())
                .await;
        }
    }

    // Delete ConfigMap
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let cm_name = format!("{name}-config");
    let _ = cm_api
        .delete(&cm_name, &kube::api::DeleteParams::default())
        .await;

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

async fn is_job_running(jobs_api: &Api<Job>, restore_name: &str) -> Result<bool> {
    let lp = kube::api::ListParams::default().labels(&format!(
        "kafkabackup.com/restore={restore_name},kafkabackup.com/type=restore"
    ));
    let jobs = jobs_api.list(&lp).await?;
    let running = jobs
        .iter()
        .any(|j| j.status.as_ref().is_some_and(|s| s.active.unwrap_or(0) > 0));
    Ok(running)
}

async fn check_job_completion(
    client: &Client,
    restore_api: &Api<KafkaRestore>,
    restore: &KafkaRestore,
    generation: i64,
) -> Result<()> {
    let name = restore.name_any();
    let namespace = restore.namespace().unwrap_or_default();
    let jobs_api: Api<Job> = Api::namespaced(client.clone(), &namespace);

    let lp = kube::api::ListParams::default().labels(&format!(
        "kafkabackup.com/restore={name},kafkabackup.com/type=restore"
    ));
    let jobs = jobs_api.list(&lp).await?;

    for job in &jobs {
        let job_name = job.metadata.name.as_deref().unwrap_or("");
        if let Some(status) = &job.status {
            if status.succeeded.unwrap_or(0) > 0 {
                info!(%job_name, "Restore job completed successfully");
                let now = Utc::now();
                let restore_info = RestoreInfo {
                    start_time: job
                        .status
                        .as_ref()
                        .and_then(|s| s.start_time.as_ref())
                        .map(|t| t.0)
                        .unwrap_or(now),
                    completion_time: Some(now),
                    status: RestoreStatus::Completed,
                    restored_topics: None,
                    restored_partitions: None,
                    restored_bytes: None,
                    point_in_time_target: None,
                    actual_point_in_time: None,
                };
                update_status_completed(restore_api, &name, generation, &restore_info).await?;
            } else if status.failed.unwrap_or(0) > 0 {
                error!(%job_name, "Restore job failed");
                update_status_error(
                    restore_api,
                    &name,
                    generation,
                    &Error::JobCreationFailed(format!("Restore job {job_name} failed")),
                )
                .await?;
            }
        }
    }

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
