use serde_yaml::Value;

use crate::crd::{KafkaBackup, KafkaRestore};
use crate::error::{Error, Result};
use crate::strimzi::kafka_cr::ResolvedKafkaCluster;
use crate::strimzi::kafka_user::ResolvedAuth;
use crate::strimzi::tls::ResolvedTlsCerts;

use super::storage_config::build_storage_config;

/// Build the complete kafka-backup config YAML for a restore operation
pub fn build_restore_config_yaml(
    restore: &KafkaRestore,
    source_backup: &KafkaBackup,
    cluster: &ResolvedKafkaCluster,
    tls_certs: &Option<ResolvedTlsCerts>,
    auth: &ResolvedAuth,
) -> Result<String> {
    let mut config = serde_yaml::Mapping::new();

    // Mode
    config.insert(
        Value::String("mode".to_string()),
        Value::String("restore".to_string()),
    );

    let backup_id = resolve_backup_id(restore, source_backup)?;
    config.insert(
        Value::String("backup_id".to_string()),
        Value::String(backup_id),
    );

    // Target (Kafka cluster)
    let target = build_kafka_config(cluster, tls_certs, auth, restore.spec.connection.as_ref())?;
    config.insert(Value::String("target".to_string()), target);

    // Storage (from source backup CR)
    let storage = build_storage_config(&source_backup.spec.storage)?;
    config.insert(Value::String("storage".to_string()), storage);

    // Restore options
    let restore_opts = build_restore_options(restore)?;
    if let Value::Mapping(ref m) = restore_opts {
        if !m.is_empty() {
            config.insert(Value::String("restore".to_string()), restore_opts);
        }
    }

    // Metrics options
    if let Some(metrics) = &restore.spec.metrics {
        let opts = build_metrics_options(metrics);
        if let Value::Mapping(ref m) = opts {
            if !m.is_empty() {
                config.insert(Value::String("metrics".to_string()), opts);
            }
        }
    }

    serde_yaml::to_string(&Value::Mapping(config)).map_err(Error::Yaml)
}

/// Build the Kafka connection config for the restore target
fn build_kafka_config(
    cluster: &ResolvedKafkaCluster,
    _tls_certs: &Option<ResolvedTlsCerts>,
    auth: &ResolvedAuth,
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

    match auth {
        ResolvedAuth::Tls { .. } => {
            security.insert(
                Value::String("ssl_certificate_location".to_string()),
                Value::String("/certs/user/user.crt".to_string()),
            );
            security.insert(
                Value::String("ssl_key_location".to_string()),
                Value::String("/certs/user/user.key".to_string()),
            );
        }
        ResolvedAuth::ScramSha512 { username, .. } => {
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

/// Build restore-specific options
fn build_restore_options(restore: &KafkaRestore) -> Result<Value> {
    let mut config = serde_yaml::Mapping::new();

    // Point-in-time recovery
    if let Some(pitr) = &restore.spec.point_in_time {
        if let Some(timestamp) = &pitr.start_timestamp {
            let dt = chrono::DateTime::parse_from_rfc3339(timestamp).map_err(|_| {
                Error::InvalidConfig(format!("Invalid startTimestamp format: {timestamp}"))
            })?;
            config.insert(
                Value::String("time_window_start".to_string()),
                Value::Number(serde_yaml::Number::from(dt.timestamp_millis())),
            );
        }
        if let Some(timestamp) = &pitr.timestamp {
            let dt = chrono::DateTime::parse_from_rfc3339(timestamp).map_err(|_| {
                Error::InvalidConfig(format!("Invalid timestamp format: {timestamp}"))
            })?;
            config.insert(
                Value::String("time_window_end".to_string()),
                Value::Number(serde_yaml::Number::from(dt.timestamp_millis())),
            );
        }
        if pitr.offset_from_end.is_some() {
            return Err(Error::InvalidConfig(
                "pointInTime.offsetFromEnd is not supported by the current kafka-backup core config"
                    .to_string(),
            ));
        }
    }

    // Topic mapping
    if !restore.spec.topic_mapping.is_empty() {
        let mut mapping = serde_yaml::Mapping::new();
        for entry in &restore.spec.topic_mapping {
            mapping.insert(
                Value::String(entry.source_topic.clone()),
                Value::String(entry.target_topic.clone()),
            );
        }
        config.insert(
            Value::String("topic_mapping".to_string()),
            Value::Mapping(mapping),
        );
    }

    if let Some(opts) = &restore.spec.restore {
        insert_bool(&mut config, "dry_run", opts.dry_run);
        insert_bool(
            &mut config,
            "include_original_offset_header",
            opts.include_original_offset_header,
        );
        if !opts.source_partitions.is_empty() {
            config.insert(
                Value::String("source_partitions".to_string()),
                Value::Sequence(
                    opts.source_partitions
                        .iter()
                        .map(|p| Value::Number(serde_yaml::Number::from(*p)))
                        .collect(),
                ),
            );
        }
        if !opts.partition_mapping.is_empty() {
            let mut mapping = serde_yaml::Mapping::new();
            for entry in &opts.partition_mapping {
                mapping.insert(
                    Value::Number(serde_yaml::Number::from(entry.source_partition)),
                    Value::Number(serde_yaml::Number::from(entry.target_partition)),
                );
            }
            config.insert(
                Value::String("partition_mapping".to_string()),
                Value::Mapping(mapping),
            );
        }
        if let Some(parallelism) = opts.parallelism {
            config.insert(
                Value::String("max_concurrent_partitions".to_string()),
                Value::Number(serde_yaml::Number::from(parallelism as i64)),
            );
        }
        insert_u64(
            &mut config,
            "rate_limit_records_per_sec",
            opts.rate_limit_records_per_sec,
        );
        insert_u64(
            &mut config,
            "rate_limit_bytes_per_sec",
            opts.rate_limit_bytes_per_sec,
        );
        if let Some(produce_batch_size) = opts.produce_batch_size {
            config.insert(
                Value::String("produce_batch_size".to_string()),
                Value::Number(serde_yaml::Number::from(produce_batch_size as u64)),
            );
        }
        if let Some(produce_acks) = opts.produce_acks {
            config.insert(
                Value::String("produce_acks".to_string()),
                Value::Number(serde_yaml::Number::from(produce_acks)),
            );
        }
        if let Some(produce_timeout_ms) = opts.produce_timeout_ms {
            config.insert(
                Value::String("produce_timeout_ms".to_string()),
                Value::Number(serde_yaml::Number::from(produce_timeout_ms)),
            );
        }
        if let Some(checkpoint_state) = &opts.checkpoint_state {
            config.insert(
                Value::String("checkpoint_state".to_string()),
                Value::String(checkpoint_state.clone()),
            );
        }
        insert_u64(
            &mut config,
            "checkpoint_interval_secs",
            opts.checkpoint_interval_secs,
        );
        if let Some(topic_creation) = &opts.topic_creation {
            insert_bool(
                &mut config,
                "create_topics",
                Some(*topic_creation == crate::crd::kafka_restore::TopicCreationPolicy::Auto),
            );
        }
        if let Some(default_replication_factor) = opts.default_replication_factor {
            config.insert(
                Value::String("default_replication_factor".to_string()),
                Value::Number(serde_yaml::Number::from(default_replication_factor)),
            );
        }
        if let Some(existing_topic_policy) = &opts.existing_topic_policy {
            match existing_topic_policy {
                crate::crd::kafka_restore::ExistingTopicPolicy::Overwrite => {
                    config.insert(Value::String("purge_topics".to_string()), Value::Bool(true));
                }
                crate::crd::kafka_restore::ExistingTopicPolicy::Append => {}
                crate::crd::kafka_restore::ExistingTopicPolicy::Fail => {
                    return Err(Error::InvalidConfig(
                        "restore.existingTopicPolicy: fail is not supported by the current kafka-backup core config".to_string(),
                    ));
                }
            }
        }
        insert_bool(&mut config, "purge_topics", opts.purge_topics);
        if !opts.repartitioning.is_empty() {
            let mut repartitioning = serde_yaml::Mapping::new();
            for entry in &opts.repartitioning {
                let mut topic_config = serde_yaml::Mapping::new();
                if let Some(strategy) = &entry.strategy {
                    topic_config.insert(
                        Value::String("strategy".to_string()),
                        Value::String(strategy.clone()),
                    );
                }
                topic_config.insert(
                    Value::String("target_partitions".to_string()),
                    Value::Number(serde_yaml::Number::from(entry.target_partitions)),
                );
                repartitioning.insert(
                    Value::String(entry.topic.clone()),
                    Value::Mapping(topic_config),
                );
            }
            config.insert(
                Value::String("repartitioning".to_string()),
                Value::Mapping(repartitioning),
            );
        }
    }

    // Consumer group restore
    if let Some(cg) = &restore.spec.consumer_groups {
        if cg.restore {
            let has_explicit_groups = !cg.mapping.is_empty() || !cg.groups.is_empty();
            let auto_groups = cg.auto.unwrap_or(false);
            if has_explicit_groups {
                config.insert(
                    Value::String("reset_consumer_offsets".to_string()),
                    Value::Bool(true),
                );
            } else if !auto_groups {
                return Err(Error::InvalidConfig(
                    "consumerGroups.restore requires consumerGroups.groups, consumerGroups.mapping, or consumerGroups.auto: true".to_string(),
                ));
            }
            if let Some(strategy) = &cg.strategy {
                config.insert(
                    Value::String("consumer_group_strategy".to_string()),
                    Value::String(strategy.clone()),
                );
            }
            if let Some(offset_report) = &cg.offset_report {
                config.insert(
                    Value::String("offset_report".to_string()),
                    Value::String(offset_report.clone()),
                );
            }
            insert_bool(&mut config, "auto_consumer_groups", cg.auto);
            if !cg.mapping.is_empty() {
                for entry in &cg.mapping {
                    if entry.source_group != entry.target_group {
                        return Err(Error::InvalidConfig(
                            "consumerGroups.mapping with different sourceGroup and targetGroup is not supported by the current kafka-backup core config".to_string(),
                        ));
                    }
                }
                config.insert(
                    Value::String("consumer_groups".to_string()),
                    Value::Sequence(
                        cg.mapping
                            .iter()
                            .map(|entry| Value::String(entry.target_group.clone()))
                            .collect(),
                    ),
                );
            } else if !cg.groups.is_empty() {
                config.insert(
                    Value::String("consumer_groups".to_string()),
                    Value::Sequence(
                        cg.groups
                            .iter()
                            .map(|group| Value::String(group.clone()))
                            .collect(),
                    ),
                );
            }
        }
    }

    Ok(Value::Mapping(config))
}

fn resolve_backup_id(restore: &KafkaRestore, source_backup: &KafkaBackup) -> Result<String> {
    if let Some(backup_id) = &restore.spec.backup_ref.backup_id {
        return Ok(backup_id.clone());
    }

    source_backup
        .status
        .as_ref()
        .and_then(|status| status.last_backup.as_ref())
        .map(|last_backup| last_backup.id.clone())
        .ok_or_else(|| {
            Error::InvalidConfig(
                "backupRef.backupId is required when the source KafkaBackup has no status.lastBackup.id"
                    .to_string(),
            )
        })
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

fn insert_bool(config: &mut serde_yaml::Mapping, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        config.insert(Value::String(key.to_string()), Value::Bool(value));
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
