use k8s_openapi::api::batch::v1::{Job, JobSpec};
use k8s_openapi::api::core::v1::{Container, PodSpec, PodTemplateSpec};
use k8s_openapi::apimachinery::pkg::apis::meta::v1::{ObjectMeta, OwnerReference};
use kube::ResourceExt;

use crate::crd::KafkaBackup;
use crate::error::Result;
use crate::reconcilers::DEFAULT_BACKUP_IMAGE;
use crate::strimzi::kafka_cr::ResolvedKafkaCluster;
use crate::strimzi::kafka_user::ResolvedAuth;

use super::templates::{
    apply_pod_template, build_annotations, build_labels, build_volumes_and_mounts,
    merge_template_labels,
};

/// Build a Kubernetes Job spec for a backup operation
pub fn build_backup_job(
    backup: &KafkaBackup,
    job_name: &str,
    config_map_name: &str,
    cluster: &ResolvedKafkaCluster,
    auth: &ResolvedAuth,
) -> Result<Job> {
    let cr_name = backup.name_any();
    let namespace = backup.namespace().unwrap_or_default();
    let image = backup.spec.image.as_deref().unwrap_or(DEFAULT_BACKUP_IMAGE);

    // Build labels
    let mut labels = build_labels(&cr_name, &cluster.name, "backup");
    merge_template_labels(&mut labels, backup.spec.template.as_ref());

    // Build annotations
    let annotations = build_annotations(backup.spec.template.as_ref());

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
        service_account_name: Some("kafka-backup-operator".to_string()),
        ..Default::default()
    };

    // Apply template overrides
    apply_pod_template(&mut pod_spec, backup.spec.template.as_ref());

    // Owner reference for garbage collection
    let owner_ref = OwnerReference {
        api_version: "kafkabackup.com/v1alpha1".to_string(),
        kind: "KafkaBackup".to_string(),
        name: cr_name.clone(),
        uid: backup.metadata.uid.clone().unwrap_or_default(),
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
                    labels: Some(build_labels(&cr_name, &cluster.name, "backup")),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crd::common::*;
    use crate::crd::kafka_backup::*;

    #[test]
    fn test_build_backup_job_basic() {
        let spec = KafkaBackupSpec {
            strimzi_cluster_ref: StrimziClusterRef {
                name: "my-cluster".to_string(),
                namespace: None,
            },
            authentication: None,
            topics: None,
            consumer_groups: None,
            storage: StorageSpec {
                storage_type: StorageType::S3,
                s3: Some(S3StorageSpec {
                    bucket: "test-bucket".to_string(),
                    region: Some("us-east-1".to_string()),
                    prefix: None,
                    endpoint: None,
                    force_path_style: None,
                    credentials_secret: Some(SecretKeyRef {
                        name: "aws-creds".to_string(),
                        key: "credentials".to_string(),
                    }),
                }),
                azure: None,
                gcs: None,
            },
            backup: None,
            schedule: None,
            retention: None,
            resources: None,
            template: None,
            image: None,
        };

        let mut backup = KafkaBackup::new("test-backup", spec);
        backup.metadata.namespace = Some("kafka".to_string());
        backup.metadata.uid = Some("test-uid".to_string());

        let cluster = ResolvedKafkaCluster {
            name: "my-cluster".to_string(),
            namespace: "kafka".to_string(),
            bootstrap_servers: "my-cluster-kafka-bootstrap:9093".to_string(),
            replicas: 3,
            tls_enabled: true,
            listener_name: "tls".to_string(),
        };

        let job = build_backup_job(
            &backup,
            "test-backup-20240101",
            "test-backup-config",
            &cluster,
            &ResolvedAuth::None,
        )
        .unwrap();

        assert_eq!(job.metadata.name.as_deref(), Some("test-backup-20240101"));
        assert_eq!(job.metadata.namespace.as_deref(), Some("kafka"));

        let labels = job.metadata.labels.as_ref().unwrap();
        assert_eq!(
            labels.get("strimzi.io/cluster"),
            Some(&"my-cluster".to_string())
        );
        assert_eq!(
            labels.get("kafkabackup.com/type"),
            Some(&"backup".to_string())
        );

        let pod_spec = job.spec.as_ref().unwrap().template.spec.as_ref().unwrap();
        assert_eq!(pod_spec.containers[0].name, "backup");
        assert_eq!(
            pod_spec.containers[0].image.as_deref(),
            Some(DEFAULT_BACKUP_IMAGE)
        );
        assert_eq!(pod_spec.restart_policy.as_deref(), Some("Never"));

        // Should have config + cluster-ca + storage-credentials volumes
        let volumes = pod_spec.volumes.as_ref().unwrap();
        assert!(volumes.iter().any(|v| v.name == "config"));
        assert!(volumes.iter().any(|v| v.name == "cluster-ca"));
        assert!(volumes.iter().any(|v| v.name == "storage-credentials"));
    }
}
