use serde_yaml::Value;

use crate::crd::KafkaBackup;
use crate::error::{Error, Result};
use crate::strimzi::kafka_cr::ResolvedKafkaCluster;
use crate::strimzi::kafka_user::ResolvedAuth;
use crate::strimzi::tls::ResolvedTlsCerts;

use super::storage_config::build_storage_config;

/// Build the complete kafka-backup config YAML from a KafkaBackup CR and resolved resources
pub fn build_backup_config_yaml(
    backup: &KafkaBackup,
    cluster: &ResolvedKafkaCluster,
    tls_certs: &Option<ResolvedTlsCerts>,
    auth: &ResolvedAuth,
) -> Result<String> {
    let mut config = serde_yaml::Mapping::new();

    // Mode
    config.insert(
        Value::String("mode".to_string()),
        Value::String("backup".to_string()),
    );

    // Backup ID is provided by the job pod via env var so scheduled jobs get unique IDs.
    config.insert(
        Value::String("backup_id".to_string()),
        Value::String("${BACKUP_ID}".to_string()),
    );

    // Source (Kafka cluster)
    let source = build_kafka_config(
        cluster,
        tls_certs,
        auth,
        backup.spec.topics.as_ref(),
        backup.spec.connection.as_ref(),
    )?;
    config.insert(Value::String("source".to_string()), source);

    // Storage
    let storage = build_storage_config(&backup.spec.storage)?;
    config.insert(Value::String("storage".to_string()), storage);

    // Backup options
    if let Some(backup_opts) = &backup.spec.backup {
        let opts = build_backup_options(backup_opts)?;
        config.insert(Value::String("backup".to_string()), opts);
    }

    // Metrics options
    if let Some(metrics) = &backup.spec.metrics {
        let opts = build_metrics_options(metrics);
        if let Value::Mapping(ref m) = opts {
            if !m.is_empty() {
                config.insert(Value::String("metrics".to_string()), opts);
            }
        }
    }

    // Offset storage
    if let Some(offset_storage) = &backup.spec.offset_storage {
        let opts = build_offset_storage_options(offset_storage);
        if let Value::Mapping(ref m) = opts {
            if !m.is_empty() {
                config.insert(Value::String("offset_storage".to_string()), opts);
            }
        }
    }

    serde_yaml::to_string(&Value::Mapping(config)).map_err(Error::Yaml)
}

/// Build the Kafka connection config section
fn build_kafka_config(
    cluster: &ResolvedKafkaCluster,
    _tls_certs: &Option<ResolvedTlsCerts>,
    auth: &ResolvedAuth,
    topics: Option<&crate::crd::common::TopicSelection>,
    connection: Option<&crate::crd::common::KafkaConnectionSpec>,
) -> Result<Value> {
    let mut kafka = serde_yaml::Mapping::new();

    kafka.insert(
        Value::String("bootstrap_servers".to_string()),
        Value::Sequence(vec![Value::String(cluster.bootstrap_servers.clone())]),
    );

    let mut security = serde_yaml::Mapping::new();
    let security_protocol = match (cluster.tls_enabled, auth) {
        (_, ResolvedAuth::Tls { .. }) => "SSL",
        (true, ResolvedAuth::ScramSha512 { .. }) => "SASL_SSL",
        (false, ResolvedAuth::ScramSha512 { .. }) => "SASL_PLAINTEXT",
        (true, ResolvedAuth::None) => "SSL",
        (false, ResolvedAuth::None) => "PLAINTEXT",
    };
    security.insert(
        Value::String("security_protocol".to_string()),
        Value::String(security_protocol.to_string()),
    );

    if cluster.tls_enabled || matches!(auth, ResolvedAuth::Tls { .. }) {
        security.insert(
            Value::String("ssl_ca_location".to_string()),
            Value::String("/certs/cluster-ca/ca.crt".to_string()),
        );
    }

    // Authentication
    match auth {
        ResolvedAuth::Tls { secret_name: _ } => {
            security.insert(
                Value::String("ssl_certificate_location".to_string()),
                Value::String("/certs/user/user.crt".to_string()),
            );
            security.insert(
                Value::String("ssl_key_location".to_string()),
                Value::String("/certs/user/user.key".to_string()),
            );
        }
        ResolvedAuth::ScramSha512 {
            username,
            secret_name: _,
            password_key: _,
        } => {
            security.insert(
                Value::String("sasl_mechanism".to_string()),
                Value::String("SCRAM-SHA-512".to_string()),
            );
            security.insert(
                Value::String("sasl_username".to_string()),
                Value::String(username.clone()),
            );
            security.insert(
                Value::String("sasl_password".to_string()),
                Value::String("${KAFKA_SASL_PASSWORD}".to_string()),
            );
        }
        ResolvedAuth::None => {}
    }

    kafka.insert(
        Value::String("security".to_string()),
        Value::Mapping(security),
    );

    if let Some(topics) = topics {
        let topic_config = build_topic_selection(topics);
        if let Value::Mapping(ref m) = topic_config {
            if !m.is_empty() {
                kafka.insert(Value::String("topics".to_string()), topic_config);
            }
        }
    }

    if let Some(connection) = connection {
        let connection_config = build_connection_config(connection);
        if let Value::Mapping(ref m) = connection_config {
            if !m.is_empty() {
                kafka.insert(Value::String("connection".to_string()), connection_config);
            }
        }
    }

    Ok(Value::Mapping(kafka))
}

/// Build backup options section
fn build_backup_options(opts: &crate::crd::kafka_backup::BackupOptionsSpec) -> Result<Value> {
    let mut config = serde_yaml::Mapping::new();

    if opts.encryption.as_ref().is_some_and(|e| e.enabled) {
        return Err(Error::InvalidConfig(
            "backup.encryption is not supported by the current kafka-backup core config"
                .to_string(),
        ));
    }

    if let Some(compression) = &opts.compression {
        config.insert(
            Value::String("compression".to_string()),
            Value::String(compression.clone()),
        );
    }

    insert_i32(&mut config, "compression_level", opts.compression_level);

    if let Some(segment_size) = opts.segment_size {
        config.insert(
            Value::String("segment_max_bytes".to_string()),
            Value::Number(serde_yaml::Number::from(segment_size)),
        );
    }

    insert_u64(
        &mut config,
        "segment_max_interval_ms",
        opts.segment_max_interval_ms,
    );

    if let Some(parallelism) = opts.parallelism {
        config.insert(
            Value::String("max_concurrent_partitions".to_string()),
            Value::Number(serde_yaml::Number::from(parallelism as i64)),
        );
    }

    if let Some(start_offset) = &opts.start_offset {
        config.insert(
            Value::String("start_offset".to_string()),
            Value::String(start_offset.clone()),
        );
    }

    insert_bool(&mut config, "continuous", opts.continuous);
    insert_bool(
        &mut config,
        "include_internal_topics",
        opts.include_internal_topics,
    );
    if !opts.internal_topics.is_empty() {
        config.insert(
            Value::String("internal_topics".to_string()),
            Value::Sequence(
                opts.internal_topics
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
    }
    insert_u64(
        &mut config,
        "checkpoint_interval_secs",
        opts.checkpoint_interval_secs,
    );
    insert_u64(&mut config, "sync_interval_secs", opts.sync_interval_secs);
    insert_bool(
        &mut config,
        "include_offset_headers",
        opts.include_offset_headers,
    );
    if let Some(source_cluster_id) = &opts.source_cluster_id {
        config.insert(
            Value::String("source_cluster_id".to_string()),
            Value::String(source_cluster_id.clone()),
        );
    }
    insert_bool(
        &mut config,
        "stop_at_current_offsets",
        opts.stop_at_current_offsets,
    );
    insert_u64(&mut config, "poll_interval_ms", opts.poll_interval_ms);
    insert_bool(
        &mut config,
        "consumer_group_snapshot",
        opts.consumer_group_snapshot,
    );

    Ok(Value::Mapping(config))
}

fn build_topic_selection(topics: &crate::crd::common::TopicSelection) -> Value {
    let mut topic_config = serde_yaml::Mapping::new();
    if !topics.include.is_empty() {
        topic_config.insert(
            Value::String("include".to_string()),
            Value::Sequence(
                topics
                    .include
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
    }
    if !topics.exclude.is_empty() {
        topic_config.insert(
            Value::String("exclude".to_string()),
            Value::Sequence(
                topics
                    .exclude
                    .iter()
                    .map(|s| Value::String(s.clone()))
                    .collect(),
            ),
        );
    }
    Value::Mapping(topic_config)
}

fn build_connection_config(connection: &crate::crd::common::KafkaConnectionSpec) -> Value {
    let mut config = serde_yaml::Mapping::new();
    insert_bool(&mut config, "tcp_keepalive", connection.tcp_keepalive);
    insert_u64(
        &mut config,
        "keepalive_time_secs",
        connection.keepalive_time_secs,
    );
    insert_u64(
        &mut config,
        "keepalive_interval_secs",
        connection.keepalive_interval_secs,
    );
    insert_bool(&mut config, "tcp_nodelay", connection.tcp_nodelay);
    if let Some(connections_per_broker) = connection.connections_per_broker {
        config.insert(
            Value::String("connections_per_broker".to_string()),
            Value::Number(serde_yaml::Number::from(connections_per_broker as u64)),
        );
    }
    Value::Mapping(config)
}

fn build_metrics_options(metrics: &crate::crd::common::MetricsSpec) -> Value {
    let mut config = serde_yaml::Mapping::new();
    insert_bool(&mut config, "enabled", metrics.enabled);
    if let Some(port) = metrics.port {
        config.insert(
            Value::String("port".to_string()),
            Value::Number(serde_yaml::Number::from(port)),
        );
    }
    if let Some(bind_address) = &metrics.bind_address {
        config.insert(
            Value::String("bind_address".to_string()),
            Value::String(bind_address.clone()),
        );
    }
    if let Some(path) = &metrics.path {
        config.insert(
            Value::String("path".to_string()),
            Value::String(path.clone()),
        );
    }
    insert_u64(
        &mut config,
        "update_interval_ms",
        metrics.update_interval_ms,
    );
    if let Some(max_partition_labels) = metrics.max_partition_labels {
        config.insert(
            Value::String("max_partition_labels".to_string()),
            Value::Number(serde_yaml::Number::from(max_partition_labels as u64)),
        );
    }
    Value::Mapping(config)
}

fn build_offset_storage_options(offset_storage: &crate::crd::common::OffsetStorageSpec) -> Value {
    let mut config = serde_yaml::Mapping::new();
    if let Some(backend) = &offset_storage.backend {
        config.insert(
            Value::String("backend".to_string()),
            Value::String(backend.clone()),
        );
    }
    if let Some(db_path) = &offset_storage.db_path {
        config.insert(
            Value::String("db_path".to_string()),
            Value::String(db_path.clone()),
        );
    }
    if let Some(s3_key) = &offset_storage.s3_key {
        config.insert(
            Value::String("s3_key".to_string()),
            Value::String(s3_key.clone()),
        );
    }
    insert_u64(
        &mut config,
        "sync_interval_secs",
        offset_storage.sync_interval_secs,
    );
    Value::Mapping(config)
}

fn insert_bool(config: &mut serde_yaml::Mapping, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        config.insert(Value::String(key.to_string()), Value::Bool(value));
    }
}

fn insert_i32(config: &mut serde_yaml::Mapping, key: &str, value: Option<i32>) {
    if let Some(value) = value {
        config.insert(
            Value::String(key.to_string()),
            Value::Number(serde_yaml::Number::from(value)),
        );
    }
}

fn insert_u64(config: &mut serde_yaml::Mapping, key: &str, value: Option<u64>) {
    if let Some(value) = value {
        config.insert(
            Value::String(key.to_string()),
            Value::Number(serde_yaml::Number::from(value)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crd::common::*;
    use crate::crd::kafka_backup::*;

    fn test_backup() -> KafkaBackup {
        let spec = KafkaBackupSpec {
            strimzi_cluster_ref: StrimziClusterRef {
                name: "my-cluster".to_string(),
                namespace: None,
            },
            authentication: None,
            topics: Some(TopicSelection {
                include: vec!["orders.*".to_string()],
                exclude: vec!["__.*".to_string()],
            }),
            connection: None,
            storage: StorageSpec {
                storage_type: StorageType::S3,
                s3: Some(S3StorageSpec {
                    bucket: "test-bucket".to_string(),
                    region: Some("us-east-1".to_string()),
                    prefix: None,
                    endpoint: None,
                    force_path_style: None,
                    allow_http: None,
                    credentials_secret: None,
                    access_key_secret: None,
                    secret_key_secret: None,
                }),
                azure: None,
                gcs: None,
                filesystem: None,
            },
            backup: Some(BackupOptionsSpec {
                compression: Some("zstd".to_string()),
                compression_level: None,
                encryption: None,
                segment_size: Some(268435456),
                segment_max_interval_ms: None,
                parallelism: Some(4),
                start_offset: None,
                continuous: None,
                include_internal_topics: None,
                internal_topics: Vec::new(),
                checkpoint_interval_secs: None,
                sync_interval_secs: None,
                include_offset_headers: None,
                source_cluster_id: None,
                stop_at_current_offsets: None,
                poll_interval_ms: None,
                consumer_group_snapshot: None,
            }),
            metrics: None,
            offset_storage: None,
            schedule: None,
            retention: None,
            resources: None,
            template: None,
            image: None,
            consumer_groups: None,
        };
        let mut backup = KafkaBackup::new("test-backup", spec);
        backup.metadata.namespace = Some("kafka".to_string());
        backup
    }

    #[test]
    fn test_build_backup_config() {
        let backup = test_backup();
        let cluster = ResolvedKafkaCluster {
            name: "my-cluster".to_string(),
            namespace: "kafka".to_string(),
            bootstrap_servers: "my-cluster-kafka-bootstrap.kafka.svc:9093".to_string(),
            replicas: 3,
            tls_enabled: true,
            listener_name: "tls".to_string(),
        };

        let yaml = build_backup_config_yaml(&backup, &cluster, &None, &ResolvedAuth::None).unwrap();
        assert!(yaml.contains("mode: backup"));
        assert!(yaml.contains("bootstrap_servers:"));
        assert!(yaml.contains("orders.*"));
    }
}
