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

    // Backup ID (generated from CR name + timestamp placeholder — actual ID set at runtime)
    config.insert(
        Value::String("backup_id".to_string()),
        Value::String(format!(
            "{}-{{timestamp}}",
            backup.metadata.name.as_deref().unwrap_or("backup")
        )),
    );

    // Source (Kafka cluster)
    let source = build_kafka_config(cluster, tls_certs, auth)?;
    config.insert(Value::String("source".to_string()), source);

    // Storage
    let storage = build_storage_config(&backup.spec.storage)?;
    config.insert(Value::String("storage".to_string()), storage);

    // Backup options
    if let Some(backup_opts) = &backup.spec.backup {
        let opts = build_backup_options(backup_opts)?;
        config.insert(Value::String("backup".to_string()), opts);
    }

    // Topic selection
    if let Some(topics) = &backup.spec.topics {
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
        config.insert(
            Value::String("topics".to_string()),
            Value::Mapping(topic_config),
        );
    }

    serde_yaml::to_string(&Value::Mapping(config)).map_err(Error::Yaml)
}

/// Build the Kafka connection config section
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

    // TLS config
    if cluster.tls_enabled {
        let mut tls = serde_yaml::Mapping::new();
        tls.insert(Value::String("enabled".to_string()), Value::Bool(true));
        // CA cert path (mounted from Strimzi secret)
        tls.insert(
            Value::String("ca_cert_path".to_string()),
            Value::String("/certs/cluster-ca/ca.crt".to_string()),
        );
        kafka.insert(Value::String("tls".to_string()), Value::Mapping(tls));
    }

    // Authentication
    match auth {
        ResolvedAuth::Tls { secret_name: _ } => {
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
        ResolvedAuth::ScramSha512 {
            username,
            secret_name: _,
            password_key: _,
        } => {
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

/// Build backup options section
fn build_backup_options(opts: &crate::crd::kafka_backup::BackupOptionsSpec) -> Result<Value> {
    let mut config = serde_yaml::Mapping::new();

    if let Some(compression) = &opts.compression {
        config.insert(
            Value::String("compression".to_string()),
            Value::String(compression.clone()),
        );
    }

    if let Some(segment_size) = opts.segment_size {
        config.insert(
            Value::String("segment_max_bytes".to_string()),
            Value::Number(serde_yaml::Number::from(segment_size)),
        );
    }

    if let Some(parallelism) = opts.parallelism {
        config.insert(
            Value::String("max_concurrent_partitions".to_string()),
            Value::Number(serde_yaml::Number::from(parallelism as i64)),
        );
    }

    Ok(Value::Mapping(config))
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
            storage: StorageSpec {
                storage_type: StorageType::S3,
                s3: Some(S3StorageSpec {
                    bucket: "test-bucket".to_string(),
                    region: Some("us-east-1".to_string()),
                    prefix: None,
                    endpoint: None,
                    force_path_style: None,
                    credentials_secret: None,
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
