use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{
    AuthenticationSpec, Condition, KafkaConnectionSpec, MetricsSpec, PodTemplateSpec,
    ResourceRequirementsSpec, RestoreInfo, StrimziClusterRef,
};

/// KafkaRestore defines a restore operation from a KafkaBackup to a Strimzi-managed Kafka cluster.
/// Supports point-in-time recovery, topic mapping, and consumer group offset restore.
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "kafkabackup.com",
    version = "v1alpha1",
    kind = "KafkaRestore",
    plural = "kafkarestores",
    shortname = "kr",
    status = "KafkaRestoreStatus",
    namespaced,
    printcolumn = r#"{"name":"Cluster","type":"string","jsonPath":".spec.strimziClusterRef.name"}"#,
    printcolumn = r#"{"name":"Backup","type":"string","jsonPath":".spec.backupRef.name"}"#,
    printcolumn = r#"{"name":"Status","type":"string","jsonPath":".status.conditions[?(@.type==\"Ready\")].reason"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct KafkaRestoreSpec {
    /// Reference to the target Strimzi Kafka cluster CR
    pub strimzi_cluster_ref: StrimziClusterRef,

    /// Authentication for the target cluster
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<AuthenticationSpec>,

    /// Reference to the source backup
    pub backup_ref: BackupRef,

    /// Point-in-time recovery settings
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_in_time: Option<PointInTimeSpec>,

    /// Kafka connection tuning for the target cluster
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connection: Option<KafkaConnectionSpec>,

    /// Topic mapping for renaming topics during restore
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topic_mapping: Vec<TopicMappingEntry>,

    /// Consumer group restore configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer_groups: Option<ConsumerGroupRestoreSpec>,

    /// Restore behaviour options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore: Option<RestoreOptionsSpec>,

    /// Metrics configuration for restore job pods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<MetricsSpec>,

    /// Resource requirements for restore pods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirementsSpec>,

    /// Template for customizing restore pods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<PodTemplateSpec>,

    /// Container image for the restore job (default: ghcr.io/osodevops/kafka-backup:latest)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

/// Reference to a KafkaBackup CR and optional specific backup snapshot
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackupRef {
    /// Name of the KafkaBackup CR
    pub name: String,
    /// Specific backup ID to restore from (latest if omitted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
}

/// Point-in-time recovery specification
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PointInTimeSpec {
    /// Start timestamp for the restore window (ISO 8601 with millisecond precision)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_timestamp: Option<String>,
    /// Exact timestamp to restore to (ISO 8601 with millisecond precision)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<String>,
    /// Duration offset from backup end (not supported by the current kafka-backup core)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_from_end: Option<String>,
}

/// Mapping for renaming a topic during restore
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopicMappingEntry {
    /// Source topic name in the backup
    pub source_topic: String,
    /// Target topic name in the destination cluster
    pub target_topic: String,
}

/// Consumer group restore configuration
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsumerGroupRestoreSpec {
    /// Whether to restore consumer group offsets
    #[serde(default)]
    pub restore: bool,
    /// Automatically load the consumer group snapshot from backup storage
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto: Option<bool>,
    /// Consumer groups to reset
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<String>,
    /// Offset handling strategy: skip, header-based, timestamp-based, cluster-scan, manual
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    /// Output file path for offset mapping report
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset_report: Option<String>,
    /// Consumer group mappings (source → target)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mapping: Vec<ConsumerGroupMapping>,
}

/// Mapping for a consumer group during restore
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsumerGroupMapping {
    /// Source consumer group name
    pub source_group: String,
    /// Target consumer group name
    pub target_group: String,
}

/// Restore behaviour options
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOptionsSpec {
    /// Topic creation strategy: auto or manual
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topic_creation: Option<TopicCreationPolicy>,
    /// Policy for existing topics: append or overwrite. fail is accepted for legacy CRs but rejected at runtime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub existing_topic_policy: Option<ExistingTopicPolicy>,
    /// Dry-run mode: validate without writing records
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dry_run: Option<bool>,
    /// Include original offsets as headers on restored records
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_original_offset_header: Option<bool>,
    /// Source partitions to restore
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_partitions: Vec<i32>,
    /// Partition mapping entries
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub partition_mapping: Vec<PartitionMappingEntry>,
    /// Number of concurrent restore threads (default: 4)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallelism: Option<i32>,
    /// Rate limit in records per second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_records_per_sec: Option<u64>,
    /// Rate limit in bytes per second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_bytes_per_sec: Option<u64>,
    /// Produce batch size
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produce_batch_size: Option<usize>,
    /// Producer acks setting (-1, 0, or 1)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produce_acks: Option<i16>,
    /// Broker-side produce timeout in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produce_timeout_ms: Option<i32>,
    /// Checkpoint state file path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_state: Option<String>,
    /// Checkpoint interval in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_interval_secs: Option<u64>,
    /// Replication factor for auto-created topics
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_replication_factor: Option<i16>,
    /// Per-topic repartitioning configuration
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repartitioning: Vec<TopicRepartitioningSpec>,
    /// Purge target topics before restoring
    #[serde(skip_serializing_if = "Option::is_none")]
    pub purge_topics: Option<bool>,
}

/// Mapping for source partition to target partition.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PartitionMappingEntry {
    /// Source partition number
    pub source_partition: i32,
    /// Target partition number
    pub target_partition: i32,
}

/// Per-topic repartitioning configuration keyed by target topic name.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopicRepartitioningSpec {
    /// Target topic name
    pub topic: String,
    /// Partitioning strategy: murmur2 or automatic
    #[serde(skip_serializing_if = "Option::is_none")]
    pub strategy: Option<String>,
    /// Target partition count
    pub target_partitions: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum TopicCreationPolicy {
    Auto,
    Manual,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ExistingTopicPolicy {
    Fail,
    Append,
    Overwrite,
}

/// Status of a KafkaRestore resource (follows Strimzi conventions)
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KafkaRestoreStatus {
    /// Strimzi-convention status conditions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,

    /// Details of the restore operation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restore: Option<RestoreInfo>,

    /// Generation observed by the operator
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
}
