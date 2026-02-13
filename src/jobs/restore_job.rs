use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use kube::ResourceExt;

use crate::crd::{KafkaBackup, KafkaRestore};
use crate::error::Result;
use crate::reconcilers::DEFAULT_BACKUP_IMAGE;
use crate::strimzi::kafka_cr::ResolvedKafkaCluster;
use crate::strimzi::kafka_user::ResolvedAuth;

use super::templates::{
    apply_pod_template, build_annotations, build_labels, build_volumes_and_mounts,
    merge_template_labels,
};

/// Build a Kubernetes Job spec for a restore operation
pub fn build_restore_job(
    restore: &KafkaRestore,
    job_name: &str,
    config_map_name: &str,
    cluster: &ResolvedKafkaCluster,
    auth: &ResolvedAuth,
    source_backup: &KafkaBackup,
) -> Result<Job> {
    let cr_name = restore.name_any();
    let namespace = restore.namespace().unwrap_or_default();
    let image = restore
        .spec
        .image
        .as_deref()
        .unwrap_or(DEFAULT_BACKUP_IMAGE);

    // Build labels
    let mut labels = build_labels(&cr_name, &cluster.name, "restore");
    merge_template_labels(&mut labels, restore.spec.template.as_ref());

    // Build annotations
    let annotations = build_annotations(restore.spec.template.as_ref());

    // Build volumes and mounts — use source backup's storage config for credentials
    let (volumes, volume_mounts) = build_volumes_and_mounts(
        config_map_name,
        "restore.yaml",
        &cluster.name,
        auth,
        &source_backup.spec.storage,
    );

    // Build container
    let container = Container {
        name: "restore".to_string(),
        image: Some(image.to_string()),
        command: Some(vec!["kafka-backup".to_string()]),
        args: Some(vec![
            "restore".to_string(),
            "--config".to_string(),
            "/config/restore.yaml".to_string(),
        ]),
        volume_mounts: Some(volume_mounts),
        resources: restore.spec.resources.as_ref().map(|r| r.to_k8s()),
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
    apply_pod_template(&mut pod_spec, restore.spec.template.as_ref());

    // Owner reference for garbage collection
    let owner_ref = OwnerReference {
        api_version: "backup.strimzi.io/v1alpha1".to_string(),
        kind: "KafkaRestore".to_string(),
        name: cr_name.clone(),
        uid: restore.metadata.uid.clone().unwrap_or_default(),
        controller: Some(true),
        block_owner_deletion: Some(true),
    };

    let job = Job {
        metadata: ObjectMeta {
            name: Some(job_name.to_string()),
            namespace: Some(namespace),
            labels: Some(labels),
            annotations: if annotations.is_empty() {
                None
            } else {
                Some(annotations)
            },
            owner_references: Some(vec![owner_ref]),
            ..Default::default()
        },
        spec: Some(JobSpec {
            backoff_limit: Some(3),
            template: PodTemplateSpec {
                metadata: Some(ObjectMeta {
                    labels: Some(build_labels(&cr_name, &cluster.name, "restore")),
                    ..Default::default()
                }),
                spec: Some(pod_spec),
            },
            ..Default::default()
        }),
        ..Default::default()
    };

    Ok(job)
}
