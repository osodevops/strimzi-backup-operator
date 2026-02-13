use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::common::{
    AuthenticationSpec, BackupHistoryEntry, Condition, ConsumerGroupSelection, LastBackupInfo,
    PodTemplateSpec, ResourceRequirementsSpec, SecretKeyRef, StorageSpec, StrimziClusterRef,
    TopicSelection,
};

/// KafkaBackup defines a backup configuration for a Strimzi-managed Kafka cluster.
/// The operator creates Kubernetes Jobs that run the kafka-backup CLI to perform backups.
#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "backup.strimzi.io",
    version = "v1alpha1",
    kind = "KafkaBackup",
    plural = "kafkabackups",
    shortname = "kb",
    status = "KafkaBackupStatus",
    namespaced,
    printcolumn = r#"{"name":"Cluster","type":"string","jsonPath":".spec.strimziClusterRef.name"}"#,
    printcolumn = r#"{"name":"Schedule","type":"string","jsonPath":".spec.schedule.cron"}"#,
    printcolumn = r#"{"name":"Last Backup","type":"date","jsonPath":".status.lastBackup.completionTime"}"#,
    printcolumn = r#"{"name":"Ready","type":"string","jsonPath":".status.conditions[?(@.type==\"Ready\")].status"}"#,
    printcolumn = r#"{"name":"Age","type":"date","jsonPath":".metadata.creationTimestamp"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct KafkaBackupSpec {
    /// Reference to the Strimzi Kafka cluster CR
    pub strimzi_cluster_ref: StrimziClusterRef,

    /// Authentication configuration for connecting to the Kafka cluster
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authentication: Option<AuthenticationSpec>,

    /// Topic selection with include/exclude regex patterns
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics: Option<TopicSelection>,

    /// Consumer group selection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub consumer_groups: Option<ConsumerGroupSelection>,

    /// Storage destination configuration
    pub storage: StorageSpec,

    /// Backup options (compression, encryption, parallelism)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup: Option<BackupOptionsSpec>,

    /// Cron schedule configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schedule: Option<ScheduleSpec>,

    /// Retention policy for managing old backups
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retention: Option<RetentionSpec>,

    /// Resource requirements for backup pods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<ResourceRequirementsSpec>,

    /// Template for customizing backup pods
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<PodTemplateSpec>,

    /// Container image for the backup job (default: ghcr.io/osodevops/kafka-backup:latest)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

/// Backup-specific options
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackupOptionsSpec {
    /// Compression algorithm: none, gzip, snappy, lz4, zstd
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression: Option<String>,

    /// Encryption configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub encryption: Option<EncryptionSpec>,

    /// Maximum segment file size in bytes before rotation (default: 256MB)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub segment_size: Option<i64>,

    /// Number of concurrent partition backup threads (default: 4)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallelism: Option<i32>,
}

/// Encryption configuration for backups
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EncryptionSpec {
    /// Enable encryption
    #[serde(default)]
    pub enabled: bool,
    /// Secret containing the encryption key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_secret: Option<SecretKeyRef>,
}

/// Cron schedule for periodic backups
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleSpec {
    /// Cron expression (e.g., "0 2 * * *" for daily at 2 AM)
    pub cron: String,
    /// Timezone (default: UTC)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// Suspend scheduling
    #[serde(default)]
    pub suspend: bool,
}

/// Retention policy for backup management
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RetentionSpec {
    /// Maximum number of backups to retain
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_backups: Option<i32>,
    /// Maximum age of backups (e.g., "30d", "720h")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_age: Option<String>,
    /// Automatically prune expired backups after each scheduled run
    #[serde(default)]
    pub prune_on_schedule: bool,
}

/// Status of a KafkaBackup resource (follows Strimzi conventions)
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KafkaBackupStatus {
    /// Strimzi-convention status conditions
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,

    /// Details of the last backup
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_backup: Option<LastBackupInfo>,

    /// History of recent backups
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backup_history: Vec<BackupHistoryEntry>,

    /// Generation observed by the operator
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,

    /// Next scheduled backup time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_scheduled_backup: Option<String>,
}
