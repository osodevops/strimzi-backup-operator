use kafka_backup_operator::adapters::restore_config::build_restore_config_yaml;
use kafka_backup_operator::crd::common::*;
use kafka_backup_operator::crd::kafka_backup::*;
use kafka_backup_operator::crd::kafka_restore::*;
use kafka_backup_operator::crd::{KafkaBackup, KafkaRestore};
use kafka_backup_operator::jobs::restore_job::build_restore_job;
use kafka_backup_operator::strimzi::kafka_cr::ResolvedKafkaCluster;
use kafka_backup_operator::strimzi::kafka_user::ResolvedAuth;

fn sample_backup() -> KafkaBackup {
    let spec = KafkaBackupSpec {
        strimzi_cluster_ref: StrimziClusterRef {
            name: "production-cluster".to_string(),
            namespace: None,
        },
        authentication: None,
        topics: None,
        consumer_groups: None,
        storage: StorageSpec {
            storage_type: StorageType::S3,
            s3: Some(S3StorageSpec {
                bucket: "my-kafka-backups".to_string(),
                region: Some("eu-west-1".to_string()),
                prefix: Some("strimzi/production-cluster/".to_string()),
                endpoint: None,
                force_path_style: None,
                credentials_secret: Some(SecretKeyRef {
                    name: "backup-s3-credentials".to_string(),
                    key: "aws-credentials".to_string(),
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
    let mut backup = KafkaBackup::new("daily-backup", spec);
    backup.metadata.namespace = Some("kafka".to_string());
    backup
}

fn sample_restore() -> KafkaRestore {
    let spec = KafkaRestoreSpec {
        strimzi_cluster_ref: StrimziClusterRef {
            name: "dr-cluster".to_string(),
            namespace: None,
        },
        authentication: None,
        backup_ref: BackupRef {
            name: "daily-backup".to_string(),
            backup_id: Some("backup-20260213-020000".to_string()),
        },
        point_in_time: Some(PointInTimeSpec {
            timestamp: Some("2026-02-13T01:30:00.000Z".to_string()),
            offset_from_end: None,
        }),
        topic_mapping: vec![
            TopicMappingEntry {
                source_topic: "orders".to_string(),
                target_topic: "orders-restored".to_string(),
            },
            TopicMappingEntry {
                source_topic: "payments".to_string(),
                target_topic: "payments-restored".to_string(),
            },
        ],
        consumer_groups: Some(ConsumerGroupRestoreSpec {
            restore: true,
            mapping: vec![ConsumerGroupMapping {
                source_group: "order-processor".to_string(),
                target_group: "order-processor".to_string(),
            }],
        }),
        restore: Some(RestoreOptionsSpec {
            topic_creation: Some(TopicCreationPolicy::Auto),
            existing_topic_policy: Some(ExistingTopicPolicy::Fail),
            parallelism: Some(4),
        }),
        resources: None,
        template: None,
        image: None,
    };
    let mut restore = KafkaRestore::new("pitr-restore", spec);
    restore.metadata.namespace = Some("kafka".to_string());
    restore.metadata.uid = Some("restore-uid-12345".to_string());
    restore
}

fn sample_cluster() -> ResolvedKafkaCluster {
    ResolvedKafkaCluster {
        name: "dr-cluster".to_string(),
        namespace: "kafka".to_string(),
        bootstrap_servers: "dr-cluster-kafka-bootstrap.kafka.svc:9093".to_string(),
        replicas: 3,
        tls_enabled: true,
        listener_name: "tls".to_string(),
    }
}

#[test]
fn test_restore_config_generation() {
    let restore = sample_restore();
    let backup = sample_backup();
    let cluster = sample_cluster();

    let yaml =
        build_restore_config_yaml(&restore, &backup, &cluster, &None, &ResolvedAuth::None).unwrap();

    assert!(yaml.contains("mode: restore"));
    assert!(yaml.contains("backup_id: backup-20260213-020000"));
    assert!(yaml.contains("bootstrap_servers: dr-cluster-kafka-bootstrap.kafka.svc:9093"));
    assert!(yaml.contains("orders: orders-restored"));
    assert!(yaml.contains("payments: payments-restored"));
    // PITR timestamp should be converted to epoch ms
    assert!(yaml.contains("time_window_end:"));
}

#[test]
fn test_restore_job_creation() {
    let restore = sample_restore();
    let backup = sample_backup();
    let cluster = sample_cluster();

    let job = build_restore_job(
        &restore,
        "pitr-restore-20260213-093000",
        "pitr-restore-config",
        &cluster,
        &ResolvedAuth::None,
        &backup,
    )
    .unwrap();

    let metadata = &job.metadata;
    assert_eq!(
        metadata.name.as_deref(),
        Some("pitr-restore-20260213-093000")
    );
    assert_eq!(metadata.namespace.as_deref(), Some("kafka"));

    let labels = metadata.labels.as_ref().unwrap();
    assert_eq!(
        labels.get("strimzi.io/cluster"),
        Some(&"dr-cluster".to_string())
    );
    assert_eq!(
        labels.get("kafkabackup.com/type"),
        Some(&"restore".to_string())
    );

    let owner_refs = metadata.owner_references.as_ref().unwrap();
    assert_eq!(owner_refs[0].kind, "KafkaRestore");
    assert_eq!(owner_refs[0].name, "pitr-restore");

    let pod_spec = job.spec.as_ref().unwrap().template.spec.as_ref().unwrap();
    assert_eq!(pod_spec.containers[0].name, "restore");
    assert!(pod_spec.containers[0]
        .args
        .as_ref()
        .unwrap()
        .contains(&"/config/restore.yaml".to_string()));
}

#[test]
fn test_restore_with_offset_from_end() {
    let mut restore = sample_restore();
    restore.spec.point_in_time = Some(PointInTimeSpec {
        timestamp: None,
        offset_from_end: Some("2h".to_string()),
    });

    let backup = sample_backup();
    let cluster = sample_cluster();

    let yaml =
        build_restore_config_yaml(&restore, &backup, &cluster, &None, &ResolvedAuth::None).unwrap();

    assert!(yaml.contains("offset_from_end: 2h"));
}
