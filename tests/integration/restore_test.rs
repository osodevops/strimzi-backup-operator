use k8s_openapi::api::core::v1::HostAlias;
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
            ca_secret: None,
            listener: None,
        },
        authentication: None,
        topics: None,
        connection: None,
        consumer_groups: None,
        logging: None,
        env: Vec::new(),
        storage: StorageSpec {
            storage_type: StorageType::S3,
            s3: Some(S3StorageSpec {
                bucket: "my-kafka-backups".to_string(),
                region: Some("eu-west-1".to_string()),
                prefix: Some("strimzi/production-cluster/".to_string()),
                endpoint: None,
                force_path_style: None,
                allow_http: None,
                credentials_secret: Some(SecretKeyRef {
                    name: "backup-s3-credentials".to_string(),
                    key: "aws-credentials".to_string(),
                }),
                access_key_secret: None,
                secret_key_secret: None,
            }),
            azure: None,
            gcs: None,
            filesystem: None,
        },
        backup: None,
        metrics: None,
        offset_storage: None,
        schedule: None,
        retention: None,
        resources: None,
        template: None,
        image: None,
        backoff_limit: None,
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
            ca_secret: None,
            listener: None,
        },
        authentication: None,
        topics: None,
        backup_ref: BackupRef {
            name: "daily-backup".to_string(),
            backup_id: Some("backup-20260213-020000".to_string()),
        },
        point_in_time: Some(PointInTimeSpec {
            start_timestamp: None,
            timestamp: Some("2026-02-13T01:30:00.000Z".to_string()),
            offset_from_end: None,
        }),
        connection: None,
        logging: None,
        env: Vec::new(),
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
            auto: None,
            groups: Vec::new(),
            strategy: None,
            offset_report: None,
            mapping: vec![ConsumerGroupMapping {
                source_group: "order-processor".to_string(),
                target_group: "order-processor".to_string(),
            }],
        }),
        restore: Some(RestoreOptionsSpec {
            topic_creation: Some(TopicCreationPolicy::Auto),
            existing_topic_policy: Some(ExistingTopicPolicy::Append),
            dry_run: None,
            include_original_offset_header: None,
            source_partitions: Vec::new(),
            partition_mapping: Vec::new(),
            parallelism: Some(4),
            rate_limit_records_per_sec: None,
            rate_limit_bytes_per_sec: None,
            produce_batch_size: None,
            produce_acks: None,
            produce_timeout_ms: None,
            checkpoint_state: None,
            checkpoint_interval_secs: None,
            default_replication_factor: None,
            repartitioning: Vec::new(),
            purge_topics: None,
        }),
        metrics: None,
        resources: None,
        template: None,
        image: None,
        backoff_limit: None,
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

fn host_alias_template() -> PodTemplateSpec {
    PodTemplateSpec {
        pod: Some(PodOverrides {
            host_aliases: vec![HostAlias {
                ip: "10.10.0.6".to_string(),
                hostnames: Some(vec![
                    "s3.internal".to_string(),
                    "backup-storage.internal".to_string(),
                ]),
            }],
            ..Default::default()
        }),
        container: None,
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
    assert!(yaml.contains("bootstrap_servers:"));
    assert!(yaml.contains("- dr-cluster-kafka-bootstrap.kafka.svc:9093"));
    assert!(yaml.contains("security_protocol: SSL"));
    assert!(yaml.contains("backend: s3"));
    assert!(yaml.contains("orders: orders-restored"));
    assert!(yaml.contains("payments: payments-restored"));
    // PITR timestamp should be converted to epoch ms
    assert!(yaml.contains("time_window_end:"));
}

#[test]
fn test_restore_logging_config_generation() {
    let mut restore = sample_restore();
    restore.spec.logging = Some(LoggingSpec {
        level: Some("debug".to_string()),
        format: Some("text".to_string()),
        output: None,
        modules: std::collections::BTreeMap::from([(
            "kafka_backup".to_string(),
            "debug".to_string(),
        )]),
        rotation: None,
    });
    let backup = sample_backup();
    let cluster = sample_cluster();

    let yaml =
        build_restore_config_yaml(&restore, &backup, &cluster, &None, &ResolvedAuth::None).unwrap();

    assert!(yaml.contains("logging:"));
    assert!(yaml.contains("level: debug"));
    assert!(yaml.contains("format: text"));
    assert!(yaml.contains("kafka_backup: debug"));
}

#[test]
fn test_restore_job_creation() {
    let mut restore = sample_restore();
    restore.spec.env.push(serde_json::json!({
        "name": "RUST_LOG",
        "value": "kafka_backup=debug"
    }));
    let backup = sample_backup();
    let cluster = sample_cluster();

    let job = build_restore_job(
        &restore,
        "pitr-restore-20260213-093000",
        "pitr-restore-config",
        &cluster,
        &ResolvedAuth::None,
        &backup,
        Some("strimzi-backup-operator"),
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
    assert_eq!(
        pod_spec.service_account_name.as_deref(),
        Some("strimzi-backup-operator")
    );
    assert_eq!(pod_spec.containers[0].name, "restore");
    let metrics_port = pod_spec.containers[0]
        .ports
        .as_ref()
        .and_then(|ports| {
            ports
                .iter()
                .find(|port| port.name.as_deref() == Some("metrics"))
        })
        .expect("metrics-enabled restore pod should declare its scrape port");
    assert_eq!(metrics_port.container_port, 8080);
    assert_eq!(
        job.spec
            .as_ref()
            .and_then(|spec| spec.template.metadata.as_ref())
            .and_then(|metadata| metadata.labels.as_ref())
            .and_then(|labels| labels.get("kafkabackup.com/metrics"))
            .map(String::as_str),
        Some("enabled")
    );
    assert!(pod_spec.containers[0]
        .args
        .as_ref()
        .unwrap()
        .contains(&"/config/restore.yaml".to_string()));
    assert!(pod_spec.containers[0]
        .env
        .as_ref()
        .unwrap()
        .iter()
        .any(|env| env.name == "RUST_LOG" && env.value.as_deref() == Some("kafka_backup=debug")));
}

#[test]
fn test_restore_job_applies_template_host_aliases() {
    let mut restore = sample_restore();
    restore.spec.template = Some(host_alias_template());
    let backup = sample_backup();
    let cluster = sample_cluster();

    let job = build_restore_job(
        &restore,
        "pitr-restore-20260213-093000",
        "pitr-restore-config",
        &cluster,
        &ResolvedAuth::None,
        &backup,
        Some("strimzi-backup-operator"),
    )
    .unwrap();

    let pod_spec = job.spec.as_ref().unwrap().template.spec.as_ref().unwrap();
    let host_aliases = pod_spec.host_aliases.as_ref().unwrap();
    assert_eq!(host_aliases.len(), 1);
    assert_eq!(host_aliases[0].ip, "10.10.0.6");
    assert_eq!(
        host_aliases[0].hostnames.as_ref().unwrap(),
        &vec![
            "s3.internal".to_string(),
            "backup-storage.internal".to_string()
        ]
    );
}

#[test]
fn test_restore_with_offset_from_end() {
    let mut restore = sample_restore();
    restore.spec.point_in_time = Some(PointInTimeSpec {
        start_timestamp: None,
        timestamp: None,
        offset_from_end: Some("2h".to_string()),
    });

    let backup = sample_backup();
    let cluster = sample_cluster();

    let err = build_restore_config_yaml(&restore, &backup, &cluster, &None, &ResolvedAuth::None)
        .unwrap_err();

    assert!(err
        .to_string()
        .contains("pointInTime.offsetFromEnd is not supported"));
}

#[test]
fn test_restore_with_unsupported_existing_topic_fail_policy() {
    let mut restore = sample_restore();
    restore.spec.restore.as_mut().unwrap().existing_topic_policy = Some(ExistingTopicPolicy::Fail);

    let backup = sample_backup();
    let cluster = sample_cluster();

    let err = build_restore_config_yaml(&restore, &backup, &cluster, &None, &ResolvedAuth::None)
        .unwrap_err();

    assert!(err
        .to_string()
        .contains("existingTopicPolicy: fail is not supported"));
}

#[test]
fn test_restore_config_topic_selection() {
    let mut restore = sample_restore();
    restore.spec.topics = Some(TopicSelection {
        include: vec!["orders-*".to_string(), "~payments-\\d+".to_string()],
        exclude: vec!["*-internal".to_string()],
    });

    let backup = sample_backup();
    let cluster = sample_cluster();

    let yaml =
        build_restore_config_yaml(&restore, &backup, &cluster, &None, &ResolvedAuth::None).unwrap();

    let config: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    let topics = &config["target"]["topics"];
    assert_eq!(topics["include"][0].as_str(), Some("orders-*"));
    assert_eq!(topics["include"][1].as_str(), Some("~payments-\\d+"));
    assert_eq!(topics["exclude"][0].as_str(), Some("*-internal"));
}

/// Issue #35: the kafka-backup binary's config parser only accepts
/// `SCRAM-SHA512` (no hyphen before the digits); `SCRAM-SHA-512` is
/// rejected with "unknown variant".
#[test]
fn test_restore_scram_auth_emits_binary_compatible_sasl_mechanism() {
    let restore = sample_restore();
    let backup = sample_backup();
    let cluster = sample_cluster();
    let auth = ResolvedAuth::ScramSha512 {
        username: "kafka-backup".to_string(),
        secret_name: "kafka-backup".to_string(),
        password_key: "password".to_string(),
    };

    let yaml = build_restore_config_yaml(&restore, &backup, &cluster, &None, &auth).unwrap();

    let config: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    assert_eq!(
        config["target"]["security"]["sasl_mechanism"].as_str(),
        Some("SCRAM-SHA512")
    );
    assert_eq!(
        config["target"]["security"]["security_protocol"].as_str(),
        Some("SASL_SSL")
    );
}

#[test]
fn test_restore_config_omits_topic_selection_by_default() {
    let restore = sample_restore();
    let backup = sample_backup();
    let cluster = sample_cluster();

    let yaml =
        build_restore_config_yaml(&restore, &backup, &cluster, &None, &ResolvedAuth::None).unwrap();

    let config: serde_yaml::Value = serde_yaml::from_str(&yaml).unwrap();
    assert!(config["target"].get("topics").is_none());
}

#[test]
fn test_restore_job_applies_template_service_account() {
    let mut restore = sample_restore();
    restore.spec.template = Some(PodTemplateSpec {
        pod: Some(PodOverrides {
            service_account_name: Some("restore-jobs".to_string()),
            ..Default::default()
        }),
        container: None,
    });
    let backup = sample_backup();
    let cluster = sample_cluster();

    let job = build_restore_job(
        &restore,
        "pitr-restore-20260213-093000",
        "pitr-restore-config",
        &cluster,
        &ResolvedAuth::None,
        &backup,
        Some("strimzi-backup-operator"),
    )
    .unwrap();

    let pod_spec = job.spec.as_ref().unwrap().template.spec.as_ref().unwrap();
    assert_eq!(
        pod_spec.service_account_name.as_deref(),
        Some("restore-jobs")
    );
}

/// Issue #31: restores append to or purge target topics, so a retried Job pod
/// re-applies a partially completed restore. The default must be exactly one
/// attempt (backoffLimit 0); retries are opt-in via spec.backoffLimit.
#[test]
fn test_restore_job_default_backoff_limit_is_zero() {
    let restore = sample_restore();
    let job = build_restore_job(
        &restore,
        "pitr-restore-20260213-093000",
        "pitr-restore-config",
        &sample_cluster(),
        &ResolvedAuth::None,
        &sample_backup(),
        None,
    )
    .unwrap();

    assert_eq!(job.spec.as_ref().unwrap().backoff_limit, Some(0));
}

#[test]
fn test_restore_job_custom_backoff_limit() {
    let mut restore = sample_restore();
    restore.spec.backoff_limit = Some(2);
    let job = build_restore_job(
        &restore,
        "pitr-restore-20260213-093000",
        "pitr-restore-config",
        &sample_cluster(),
        &ResolvedAuth::None,
        &sample_backup(),
        None,
    )
    .unwrap();

    assert_eq!(job.spec.as_ref().unwrap().backoff_limit, Some(2));
}
