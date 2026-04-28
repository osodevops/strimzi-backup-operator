use std::sync::Arc;

use chrono::Utc;
use k8s_openapi::api::batch::v1::Job;
use k8s_openapi::api::core::v1::ConfigMap;
use kube::{
    api::{Api, Patch, PatchParams, PostParams, ResourceExt},
    Client,
};
use std::collections::BTreeSet;
use tracing::{debug, error, info, warn};

use crate::adapters::backup_config::build_backup_config_yaml;
use crate::crd::common::{BackupHistoryEntry, BackupStatus, LastBackupInfo};
use crate::crd::{KafkaBackup, KafkaBackupStatus};
use crate::error::{Error, Result};
use crate::jobs::backup_job::build_backup_job;
use crate::jobs::cronjob::build_backup_cronjob;
use crate::metrics::prometheus::MetricsState;
use crate::reconcilers::{
    job_service_account_name, FINALIZER, TRIGGER_ANNOTATION, TRIGGER_VALUE_NOW,
};
use crate::retention::policy::evaluate_retention;
use crate::retention::storage::{discover_backup_history, prune_backup_ids};
use crate::status::conditions::*;
use crate::strimzi::kafka_cr::resolve_kafka_cluster;
use crate::strimzi::kafka_user::resolve_auth;
use crate::strimzi::tls::resolve_cluster_ca;

pub async fn reconcile_backup(
    backup: Arc<KafkaBackup>,
    client: Client,
    _metrics: &MetricsState,
) -> Result<()> {
    let name = backup.name_any();
    let namespace = backup
        .namespace()
        .ok_or(Error::MissingObjectKey(".metadata.namespace"))?;
    let backup_api: Api<KafkaBackup> = Api::namespaced(client.clone(), &namespace);

    // Check if being deleted
    if backup.metadata.deletion_timestamp.is_some() {
        return handle_cleanup(&backup, &client, &namespace).await;
    }

    // Ensure finalizer is set
    if !backup
        .metadata
        .finalizers
        .as_ref()
        .is_some_and(|f| f.contains(&FINALIZER.to_string()))
    {
        add_finalizer(&backup_api, &name).await?;
    }

    // Update observed generation
    let generation = backup.metadata.generation.unwrap_or(0);

    // Step 1: Resolve Strimzi Kafka cluster
    let kafka_cluster =
        match resolve_kafka_cluster(&client, &backup.spec.strimzi_cluster_ref, &namespace).await {
            Ok(cluster) => cluster,
            Err(e) => {
                update_status_error(&backup_api, &name, generation, &e).await?;
                return Err(e);
            }
        };

    // Step 2: Resolve TLS certificates
    let tls_certs = match resolve_cluster_ca(
        &client,
        &kafka_cluster.name,
        backup.spec.strimzi_cluster_ref.ca_secret.as_ref(),
        &namespace,
    )
    .await
    {
        Ok(certs) => Some(certs),
        Err(e) => {
            warn!(%name, error = %e, "Failed to resolve TLS certs (may not be required)");
            None
        }
    };

    // Step 3: Resolve authentication
    let resolved_auth =
        resolve_auth(&client, backup.spec.authentication.as_ref(), &namespace).await?;

    // Step 4: Build config YAML and create ConfigMap
    let config_yaml =
        build_backup_config_yaml(&backup, &kafka_cluster, &tls_certs, &resolved_auth)?;
    let config_map_name = format!("{name}-config");
    create_or_update_config_map(&client, &namespace, &config_map_name, &config_yaml, &backup)
        .await?;

    // Step 5: Check for scheduled vs one-shot
    let job_service_account = job_service_account_name();
    if let Some(schedule) = &backup.spec.schedule {
        if !schedule.suspend {
            // Create CronJob
            let cronjob = build_backup_cronjob(
                &backup,
                &config_map_name,
                &kafka_cluster,
                &resolved_auth,
                job_service_account.as_deref(),
            )?;
            let cronjob_api: Api<k8s_openapi::api::batch::v1::CronJob> =
                Api::namespaced(client.clone(), &namespace);
            let cronjob_name = format!("{name}-scheduled");

            apply_resource(&cronjob_api, &cronjob_name, &cronjob).await?;

            // Update status
            let next_backup = schedule.cron.clone();
            update_status_scheduled(&backup_api, &name, generation, &next_backup).await?;

            info!(%name, cron = %schedule.cron, "CronJob created/updated for scheduled backup");
        }
    }

    // Step 6: Check for manual trigger annotation
    let triggered = backup
        .metadata
        .annotations
        .as_ref()
        .and_then(|a| a.get(TRIGGER_ANNOTATION))
        .is_some_and(|v| v == TRIGGER_VALUE_NOW);

    // Step 7: Create one-shot Job (if no schedule, or if manually triggered)
    if backup.spec.schedule.is_none() || triggered {
        let job_name = format!("{name}-{}", Utc::now().format("%Y%m%d-%H%M%S"));
        let job = build_backup_job(
            &backup,
            &job_name,
            &config_map_name,
            &kafka_cluster,
            &resolved_auth,
            job_service_account.as_deref(),
        )?;

        let jobs_api: Api<Job> = Api::namespaced(client.clone(), &namespace);

        // Check if a job is already running
        if !is_job_running(&jobs_api, &name).await? {
            jobs_api
                .create(&PostParams::default(), &job)
                .await
                .map_err(|e| Error::JobCreationFailed(e.to_string()))?;

            info!(%job_name, "Created backup job");
            update_status_running(&backup_api, &name, generation).await?;
        } else {
            debug!(%name, "Backup job already running, skipping");
        }

        // Remove trigger annotation if present
        if triggered {
            remove_trigger_annotation(&backup_api, &name).await?;
        }
    }

    // Step 8: Check running job status and update
    check_job_completion(&client, &backup_api, &backup, generation).await?;
    apply_retention_policy(&client, &backup_api, &backup, generation).await?;

    Ok(())
}

async fn handle_cleanup(backup: &KafkaBackup, client: &Client, namespace: &str) -> Result<()> {
    let name = backup.name_any();
    info!(%name, "Cleaning up KafkaBackup resources");

    // Delete associated jobs
    let jobs_api: Api<Job> = Api::namespaced(client.clone(), namespace);
    let lp = kube::api::ListParams::default().labels(&format!(
        "app.kubernetes.io/managed-by=kafka-backup-operator,kafkabackup.com/backup={name}"
    ));
    if let Ok(job_list) = jobs_api.list(&lp).await {
        for job in job_list {
            let job_name = job.metadata.name.unwrap_or_default();
            let _ = jobs_api
                .delete(&job_name, &kube::api::DeleteParams::default())
                .await;
        }
    }

    // Delete CronJob if exists
    let cronjob_api: Api<k8s_openapi::api::batch::v1::CronJob> =
        Api::namespaced(client.clone(), namespace);
    let cronjob_name = format!("{name}-scheduled");
    let _ = cronjob_api
        .delete(&cronjob_name, &kube::api::DeleteParams::default())
        .await;

    // Delete ConfigMap
    let cm_api: Api<ConfigMap> = Api::namespaced(client.clone(), namespace);
    let cm_name = format!("{name}-config");
    let _ = cm_api
        .delete(&cm_name, &kube::api::DeleteParams::default())
        .await;

    // Remove finalizer
    let backup_api: Api<KafkaBackup> = Api::namespaced(client.clone(), namespace);
    remove_finalizer(&backup_api, &name).await?;

    info!(%name, "Cleanup complete");
    Ok(())
}

async fn add_finalizer(api: &Api<KafkaBackup>, name: &str) -> Result<()> {
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

async fn remove_finalizer(api: &Api<KafkaBackup>, name: &str) -> Result<()> {
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

async fn remove_trigger_annotation(api: &Api<KafkaBackup>, name: &str) -> Result<()> {
    let patch = serde_json::json!({
        "metadata": {
            "annotations": {
                TRIGGER_ANNOTATION: null
            }
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
    owner: &KafkaBackup,
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
                "kafkabackup.com/backup": owner.name_any()
            },
            "ownerReferences": [{
                "apiVersion": "kafkabackup.com/v1alpha1",
                "kind": "KafkaBackup",
                "name": owner.name_any(),
                "uid": owner.metadata.uid.as_deref().unwrap_or(""),
                "controller": true,
                "blockOwnerDeletion": true
            }]
        },
        "data": {
            "backup.yaml": config_yaml
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

async fn is_job_running(jobs_api: &Api<Job>, backup_name: &str) -> Result<bool> {
    let lp = kube::api::ListParams::default().labels(&format!(
        "kafkabackup.com/backup={backup_name},kafkabackup.com/type=backup"
    ));
    let jobs = jobs_api.list(&lp).await?;
    let running = jobs
        .iter()
        .any(|j| j.status.as_ref().is_some_and(|s| s.active.unwrap_or(0) > 0));
    Ok(running)
}

async fn apply_retention_policy(
    client: &Client,
    backup_api: &Api<KafkaBackup>,
    backup: &KafkaBackup,
    generation: i64,
) -> Result<()> {
    let Some(retention) = &backup.spec.retention else {
        return Ok(());
    };
    if backup.spec.schedule.is_none() || !retention.prune_on_schedule {
        return Ok(());
    }

    let name = backup.name_any();
    let namespace = backup.namespace().unwrap_or_default();
    let mut history = current_backup_status(backup_api, &name)
        .await?
        .backup_history;

    let discovered =
        discover_backup_history(client, &namespace, &backup.spec.storage, &name).await?;
    merge_backup_history(&mut history, discovered);

    let active_backup_ids = active_backup_ids(client, &namespace, &name, backup).await?;
    let mut to_prune = evaluate_retention(&history, retention);
    to_prune.retain(|id| !active_backup_ids.contains(id));

    if to_prune.is_empty() {
        patch_backup_history(backup_api, &name, &history).await?;
        return Ok(());
    }

    let pruned = prune_backup_ids(client, &namespace, &backup.spec.storage, &to_prune).await?;
    history.retain(|entry| !pruned.contains(&entry.id));
    patch_backup_history(backup_api, &name, &history).await?;

    info!(
        %name,
        generation,
        pruned = pruned.len(),
        "Applied backup retention policy"
    );

    Ok(())
}

async fn active_backup_ids(
    client: &Client,
    namespace: &str,
    backup_name: &str,
    backup: &KafkaBackup,
) -> Result<BTreeSet<String>> {
    let jobs_api: Api<Job> = Api::namespaced(client.clone(), namespace);
    let lp = kube::api::ListParams::default().labels(&format!(
        "kafkabackup.com/backup={backup_name},kafkabackup.com/type=backup"
    ));
    let jobs = jobs_api.list(&lp).await?;
    let mut active = BTreeSet::new();

    for job in jobs {
        let is_active = job
            .status
            .as_ref()
            .is_some_and(|status| status.active.unwrap_or(0) > 0);
        if !is_active {
            continue;
        }

        if let Some(job_name) = job.metadata.name {
            active.insert(job_name);
        }
        if backup.spec.offset_storage.is_some() {
            active.insert(backup_name.to_string());
        }
    }

    Ok(active)
}

async fn check_job_completion(
    client: &Client,
    backup_api: &Api<KafkaBackup>,
    backup: &KafkaBackup,
    generation: i64,
) -> Result<()> {
    let name = backup.name_any();
    let namespace = backup.namespace().unwrap_or_default();
    let jobs_api: Api<Job> = Api::namespaced(client.clone(), &namespace);

    let lp = kube::api::ListParams::default().labels(&format!(
        "kafkabackup.com/backup={name},kafkabackup.com/type=backup"
    ));
    let jobs = jobs_api.list(&lp).await?;

    for job in &jobs {
        let job_name = job.metadata.name.as_deref().unwrap_or("");
        if let Some(status) = &job.status {
            if status.succeeded.unwrap_or(0) > 0 {
                info!(%job_name, "Backup job completed successfully");
                let backup_id = job_name.to_string();
                let now = Utc::now();

                let history_entry = BackupHistoryEntry {
                    id: backup_id.clone(),
                    status: BackupStatus::Completed,
                    start_time: job
                        .status
                        .as_ref()
                        .and_then(|s| s.start_time.as_ref())
                        .map(|t| t.0)
                        .unwrap_or(now),
                    completion_time: Some(now),
                    size_bytes: None,
                    topics_backed_up: None,
                    partitions_backed_up: None,
                };

                update_status_completed(backup_api, &name, generation, &history_entry).await?;
            } else if status.failed.unwrap_or(0) > 0 {
                error!(%job_name, "Backup job failed");
                update_status_error(
                    backup_api,
                    &name,
                    generation,
                    &Error::JobCreationFailed(format!("Job {job_name} failed")),
                )
                .await?;
            }
        }
    }

    Ok(())
}

async fn update_status_running(api: &Api<KafkaBackup>, name: &str, generation: i64) -> Result<()> {
    let mut status = current_backup_status(api, name).await?;
    status.conditions = vec![not_ready(REASON_BACKUP_RUNNING, "Backup job is running")];
    status.observed_generation = Some(generation);
    patch_status(api, name, &status).await
}

async fn update_status_scheduled(
    api: &Api<KafkaBackup>,
    name: &str,
    generation: i64,
    next_backup: &str,
) -> Result<()> {
    let mut status = current_backup_status(api, name).await?;
    status.conditions = vec![ready(
        REASON_BACKUP_SCHEDULED,
        &format!("Next backup scheduled: {next_backup}"),
    )];
    status.observed_generation = Some(generation);
    status.next_scheduled_backup = Some(next_backup.to_string());
    patch_status(api, name, &status).await
}

async fn update_status_completed(
    api: &Api<KafkaBackup>,
    name: &str,
    generation: i64,
    entry: &BackupHistoryEntry,
) -> Result<()> {
    let mut status = current_backup_status(api, name).await?;
    let last_backup = LastBackupInfo {
        id: entry.id.clone(),
        start_time: entry.start_time,
        completion_time: entry.completion_time,
        status: BackupStatus::Completed,
        size_bytes: entry.size_bytes,
        topics_backed_up: entry.topics_backed_up,
        partitions_backed_up: entry.partitions_backed_up,
        oldest_timestamp: None,
        newest_timestamp: None,
    };

    status.conditions = vec![ready(
        REASON_BACKUP_COMPLETED,
        "Backup completed successfully",
    )];
    status.last_backup = Some(last_backup);
    status.observed_generation = Some(generation);
    upsert_history_entry(&mut status.backup_history, entry.clone());
    patch_status(api, name, &status).await
}

async fn update_status_error(
    api: &Api<KafkaBackup>,
    name: &str,
    generation: i64,
    error: &Error,
) -> Result<()> {
    let mut status = current_backup_status(api, name).await?;
    status.conditions = error_conditions(error.reason(), &error.to_string());
    status.observed_generation = Some(generation);
    patch_status(api, name, &status).await
}

async fn current_backup_status(api: &Api<KafkaBackup>, name: &str) -> Result<KafkaBackupStatus> {
    Ok(api.get_status(name).await?.status.unwrap_or_default())
}

fn merge_backup_history(
    history: &mut Vec<BackupHistoryEntry>,
    discovered: Vec<BackupHistoryEntry>,
) {
    for entry in discovered {
        upsert_history_entry(history, entry);
    }
    history.sort_by_key(|entry| std::cmp::Reverse(entry.start_time));
}

fn upsert_history_entry(history: &mut Vec<BackupHistoryEntry>, mut entry: BackupHistoryEntry) {
    if let Some(existing) = history.iter_mut().find(|existing| existing.id == entry.id) {
        if entry.completion_time.is_none() {
            entry.completion_time = existing.completion_time;
        }
        *existing = entry;
    } else {
        history.push(entry);
    }
    history.sort_by_key(|entry| std::cmp::Reverse(entry.start_time));
}

async fn patch_backup_history(
    api: &Api<KafkaBackup>,
    name: &str,
    history: &[BackupHistoryEntry],
) -> Result<()> {
    let patch = serde_json::json!({ "status": { "backupHistory": history } });
    api.patch_status(
        name,
        &PatchParams::apply("kafka-backup-operator"),
        &Patch::Merge(&patch),
    )
    .await?;
    Ok(())
}

async fn patch_status(
    api: &Api<KafkaBackup>,
    name: &str,
    status: &KafkaBackupStatus,
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

async fn apply_resource<
    T: serde::Serialize + Clone + kube::Resource + std::fmt::Debug + serde::de::DeserializeOwned,
>(
    api: &Api<T>,
    name: &str,
    resource: &T,
) -> Result<()> {
    let patch = serde_json::to_value(resource).map_err(Error::Serialization)?;
    api.patch(
        name,
        &PatchParams::apply("kafka-backup-operator"),
        &Patch::Apply(patch),
    )
    .await?;
    Ok(())
}
