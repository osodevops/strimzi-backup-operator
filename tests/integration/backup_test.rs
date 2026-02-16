use kafka_backup_operator::adapters::backup_config::build_backup_config_yaml;
use kafka_backup_operator::crd::common::*;
use kafka_backup_operator::crd::kafka_backup::*;
use kafka_backup_operator::crd::KafkaBackup;
use kafka_backup_operator::jobs::backup_job::build_backup_job;
use kafka_backup_operator::strimzi::kafka_cr::ResolvedKafkaCluster;
use kafka_backup_operator::strimzi::kafka_user::ResolvedAuth;

fn sample_backup() -> KafkaBackup {
    let spec = KafkaBackupSpec {
        strimzi_cluster_ref: StrimziClusterRef {
            name: "production-cluster".to_string(),
            namespace: None,
        },
        authentication: None,
        topics: Some(TopicSelection {
            include: vec!["orders.*".to_string(), "payments.*".to_string()],
            exclude: vec!["__.*".to_string()],
        }),
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
        backup: Some(BackupOptionsSpec {
            compression: Some("zstd".to_string()),
            encryption: None,
            segment_size: Some(268435456),
            parallelism: Some(4),
        }),
        schedule: Some(ScheduleSpec {
            cron: "0 2 * * *".to_string(),
            timezone: Some("UTC".to_string()),
            suspend: false,
        }),
        retention: Some(RetentionSpec {
            max_backups: Some(30),
            max_age: Some("30d".to_string()),
            prune_on_schedule: true,
        }),
        resources: None,
        template: None,
        image: None,
    };
    let mut backup = KafkaBackup::new("daily-backup", spec);
    backup.metadata.namespace = Some("kafka".to_string());
    backup.metadata.uid = Some("test-uid-12345".to_string());
    backup
}

fn sample_cluster() -> ResolvedKafkaCluster {
    ResolvedKafkaCluster {
        name: "production-cluster".to_string(),
        namespace: "kafka".to_string(),
        bootstrap_servers: "production-cluster-kafka-bootstrap.kafka.svc:9093".to_string(),
        replicas: 3,
        tls_enabled: true,
        listener_name: "tls".to_string(),
    }
}

#[test]
fn test_backup_config_generation() {
    let backup = sample_backup();
    let cluster = sample_cluster();

    let yaml = build_backup_config_yaml(&backup, &cluster, &None, &ResolvedAuth::None).unwrap();

    assert!(yaml.contains("mode: backup"));
    assert!(yaml.contains("bootstrap_servers: production-cluster-kafka-bootstrap.kafka.svc:9093"));
    assert!(yaml.contains("compression: zstd"));
    assert!(yaml.contains("orders.*"));
    assert!(yaml.contains("payments.*"));
    assert!(yaml.contains("bucket: my-kafka-backups"));
}

#[test]
fn test_backup_job_creation() {
    let backup = sample_backup();
    let cluster = sample_cluster();

    let job = build_backup_job(
        &backup,
        "daily-backup-20260213-020000",
        "daily-backup-config",
        &cluster,
        &ResolvedAuth::None,
    )
    .unwrap();

    let metadata = &job.metadata;
    assert_eq!(
        metadata.name.as_deref(),
        Some("daily-backup-20260213-020000")
    );
    assert_eq!(metadata.namespace.as_deref(), Some("kafka"));

    let labels = metadata.labels.as_ref().unwrap();
    assert_eq!(
        labels.get("strimzi.io/cluster"),
        Some(&"production-cluster".to_string())
    );
    assert_eq!(
        labels.get("app.kubernetes.io/part-of"),
        Some(&"kafka-backup".to_string())
    );

    let owner_refs = metadata.owner_references.as_ref().unwrap();
    assert_eq!(owner_refs.len(), 1);
    assert_eq!(owner_refs[0].kind, "KafkaBackup");
    assert_eq!(owner_refs[0].name, "daily-backup");

    let spec = job.spec.as_ref().unwrap();
    let pod_spec = spec.template.spec.as_ref().unwrap();
    assert_eq!(pod_spec.containers.len(), 1);
    assert_eq!(pod_spec.containers[0].name, "backup");
    assert!(pod_spec.containers[0]
        .args
        .as_ref()
        .unwrap()
        .contains(&"/config/backup.yaml".to_string()));
}

#[test]
fn test_backup_with_tls_auth() {
    let mut backup = sample_backup();
    backup.spec.authentication = Some(AuthenticationSpec {
        auth_type: AuthenticationType::Tls,
        kafka_user_ref: Some(KafkaUserRef {
            name: "backup-user".to_string(),
        }),
        certificate_and_key: None,
        password_secret: None,
        username: None,
    });

    let cluster = sample_cluster();
    let auth = ResolvedAuth::Tls {
        secret_name: "backup-user".to_string(),
    };

    let yaml = build_backup_config_yaml(&backup, &cluster, &None, &auth).unwrap();
    assert!(yaml.contains("type: tls"));
    assert!(yaml.contains("cert_path: /certs/user/user.crt"));

    let job = build_backup_job(&backup, "test-job", "test-config", &cluster, &auth).unwrap();

    let volumes = job
        .spec
        .as_ref()
        .unwrap()
        .template
        .spec
        .as_ref()
        .unwrap()
        .volumes
        .as_ref()
        .unwrap();
    assert!(volumes.iter().any(|v| v.name == "user-certs"));
}
