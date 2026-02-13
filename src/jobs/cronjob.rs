use k8s_openapi::api::batch::v1::{CronJob, CronJobSpec, JobSpec, JobTemplateSpec};
use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use kube::ResourceExt;

use crate::crd::KafkaBackup;
use crate::error::Result;
use crate::reconcilers::DEFAULT_BACKUP_IMAGE;
use crate::strimzi::kafka_cr::ResolvedKafkaCluster;
use crate::strimzi::kafka_user::ResolvedAuth;

use super::templates::{
    apply_pod_template, build_labels, build_volumes_and_mounts, merge_template_labels,
};

/// Build a Kubernetes CronJob for scheduled backups
pub fn build_backup_cronjob(
    backup: &KafkaBackup,
    config_map_name: &str,
    cluster: &ResolvedKafkaCluster,
    auth: &ResolvedAuth,
) -> Result<CronJob> {
    let cr_name = backup.name_any();
    let namespace = backup.namespace().unwrap_or_default();
    let image = backup.spec.image.as_deref().unwrap_or(DEFAULT_BACKUP_IMAGE);

    let schedule = backup
        .spec
        .schedule
        .as_ref()
        .expect("CronJob requires schedule");

    // Build labels
    let mut labels = build_labels(&cr_name, &cluster.name, "backup");
    merge_template_labels(&mut labels, backup.spec.template.as_ref());

    // Build volumes and mounts
    let (volumes, volume_mounts) = build_volumes_and_mounts(
        config_map_name,
        "backup.yaml",
        &cluster.name,
        auth,
        &backup.spec.storage,
    );

    // Build container
    let container = Container {
        name: "backup".to_string(),
        image: Some(image.to_string()),
        command: Some(vec!["kafka-backup".to_string()]),
        args: Some(vec![
            "backup".to_string(),
            "--config".to_string(),
            "/config/backup.yaml".to_string(),
        ]),
        volume_mounts: Some(volume_mounts),
        resources: backup.spec.resources.as_ref().map(|r| r.to_k8s()),
        ..Default::default()
    };

    // Build pod spec
    let mut pod_spec = PodSpec {
        containers: vec![container],
        volumes: Some(volumes),
        restart_policy: Some("Never".to_string()),
        service_account_name: Some("strimzi-backup-operator".to_string()),
        ..Default::default()
    };

    // Apply template overrides
    apply_pod_template(&mut pod_spec, backup.spec.template.as_ref());

    // Owner reference
    let owner_ref = OwnerReference {
        api_version: "backup.strimzi.io/v1alpha1".to_string(),
        kind: "KafkaBackup".to_string(),
        name: cr_name.clone(),
        uid: backup.metadata.uid.clone().unwrap_or_default(),
        controller: Some(true),
        block_owner_deletion: Some(true),
    };

    let cronjob = CronJob {
        metadata: ObjectMeta {
            name: Some(format!("{cr_name}-scheduled")),
            namespace: Some(namespace),
            labels: Some(labels.clone()),
            owner_references: Some(vec![owner_ref]),
            ..Default::default()
        },
        spec: Some(CronJobSpec {
            schedule: schedule.cron.clone(),
            time_zone: schedule.timezone.clone(),
            suspend: Some(schedule.suspend),
            concurrency_policy: Some("Forbid".to_string()),
            successful_jobs_history_limit: Some(3),
            failed_jobs_history_limit: Some(3),
            job_template: JobTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(labels.clone()),
                    ..Default::default()
                }),
                spec: Some(JobSpec {
                    backoff_limit: Some(3),
                    template: PodTemplateSpec {
                        metadata: Some(ObjectMeta {
                            labels: Some(labels),
                            ..Default::default()
                        }),
                        spec: Some(pod_spec),
                    },
                    ..Default::default()
                }),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    Ok(cronjob)
}
