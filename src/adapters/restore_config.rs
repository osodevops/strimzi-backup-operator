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

    // Backup ID (from backupRef or latest)
    if let Some(backup_id) = &restore.spec.backup_ref.backup_id {
        config.insert(
            Value::String("backup_id".to_string()),
            Value::String(backup_id.clone()),
        );
    }

    // Target (Kafka cluster)
    let target = build_kafka_config(cluster, tls_certs, auth)?;
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

    // Point-in-time recovery
    if let Some(pitr) = &restore.spec.point_in_time {
        if let Some(timestamp) = &pitr.timestamp {
            // Parse ISO 8601 to epoch ms
            if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(timestamp) {
                config.insert(
                    Value::String("time_window_end".to_string()),
                    Value::Number(serde_yaml::Number::from(dt.timestamp_millis())),
                );
            } else {
                return Err(Error::InvalidConfig(format!(
                    "Invalid timestamp format: {timestamp}"
                )));
            }
        }
        if let Some(offset) = &pitr.offset_from_end {
            // Store as duration string — the CLI will handle parsing
            config.insert(
                Value::String("offset_from_end".to_string()),
                Value::String(offset.clone()),
            );
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

    serde_yaml::to_string(&Value::Mapping(config)).map_err(Error::Yaml)
}

/// Build the Kafka connection config for the restore target
fn build_kafka_config(
    cluster: &ResolvedKafkaCluster,
    _tls_certs: &Option<ResolvedTlsCerts>,
    auth: &ResolvedAuth,
) -> Result<Value> {
    let mut kafka = serde_yaml::Mapping::new();

    kafka.insert(
        Value::String("bootstrap_servers".to_string()),
        Value::String(cluster.bootstrap_servers.clone()),
    );

    if cluster.tls_enabled {
        let mut tls = serde_yaml::Mapping::new();
        tls.insert(Value::String("enabled".to_string()), Value::Bool(true));
        tls.insert(
            Value::String("ca_cert_path".to_string()),
            Value::String("/certs/cluster-ca/ca.crt".to_string()),
        );
        kafka.insert(Value::String("tls".to_string()), Value::Mapping(tls));
    }

    match auth {
        ResolvedAuth::Tls { .. } => {
            let mut auth_config = serde_yaml::Mapping::new();
            auth_config.insert(
                Value::String("type".to_string()),
                Value::String("tls".to_string()),
            );
            auth_config.insert(
                Value::String("cert_path".to_string()),
                Value::String("/certs/user/user.crt".to_string()),
            );
            auth_config.insert(
                Value::String("key_path".to_string()),
                Value::String("/certs/user/user.key".to_string()),
            );
            kafka.insert(
                Value::String("authentication".to_string()),
                Value::Mapping(auth_config),
            );
        }
        ResolvedAuth::ScramSha512 { username, .. } => {
            let mut auth_config = serde_yaml::Mapping::new();
            auth_config.insert(
                Value::String("type".to_string()),
                Value::String("scram-sha-512".to_string()),
            );
            auth_config.insert(
                Value::String("username".to_string()),
                Value::String(username.clone()),
            );
            auth_config.insert(
                Value::String("password_file".to_string()),
                Value::String("/certs/user/password".to_string()),
            );
            kafka.insert(
                Value::String("authentication".to_string()),
                Value::Mapping(auth_config),
            );
        }
        ResolvedAuth::None => {}
    }

    Ok(Value::Mapping(kafka))
}

/// Build restore-specific options
fn build_restore_options(restore: &KafkaRestore) -> Result<Value> {
    let mut config = serde_yaml::Mapping::new();

    if let Some(opts) = &restore.spec.restore {
        if let Some(parallelism) = opts.parallelism {
            config.insert(
                Value::String("max_concurrent_partitions".to_string()),
                Value::Number(serde_yaml::Number::from(parallelism as i64)),
            );
        }
    }

    // Consumer group restore
    if let Some(cg) = &restore.spec.consumer_groups {
        if cg.restore {
            config.insert(
                Value::String("restore_consumer_groups".to_string()),
                Value::Bool(true),
            );
            if !cg.mapping.is_empty() {
                let mut group_mapping = serde_yaml::Mapping::new();
                for entry in &cg.mapping {
                    group_mapping.insert(
                        Value::String(entry.source_group.clone()),
                        Value::String(entry.target_group.clone()),
                    );
                }
                config.insert(
                    Value::String("consumer_group_mapping".to_string()),
                    Value::Mapping(group_mapping),
                );
            }
        }
    }

    Ok(Value::Mapping(config))
}
