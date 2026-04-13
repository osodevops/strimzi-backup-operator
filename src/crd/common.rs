use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Resource requirements (CPU/memory) for pods — mirrors k8s ResourceRequirements with JsonSchema
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ResourceRequirementsSpec {
    /// Resource requests
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub requests: BTreeMap<String, String>,
    /// Resource limits
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub limits: BTreeMap<String, String>,
}

impl ResourceRequirementsSpec {
    /// Convert to k8s-openapi ResourceRequirements for use in pod specs
    pub fn to_k8s(&self) -> k8s_openapi::api::core::v1::ResourceRequirements {
        use k8s_openapi::apimachinery::pkg::api::resource::Quantity;
        k8s_openapi::api::core::v1::ResourceRequirements {
            requests: if self.requests.is_empty() {
                None
            } else {
                Some(
                    self.requests
                        .iter()
                        .map(|(k, v)| (k.clone(), Quantity(v.clone())))
                        .collect(),
                )
            },
            limits: if self.limits.is_empty() {
                None
            } else {
                Some(
                    self.limits
                        .iter()
                        .map(|(k, v)| (k.clone(), Quantity(v.clone())))
                        .collect(),
                )
            },
            ..Default::default()
        }
    }
}

/// Reference to a Strimzi Kafka cluster CR
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StrimziClusterRef {
    /// Name of the Kafka CR
    pub name: String,
    /// Namespace of the Kafka CR (defaults to same namespace as this resource)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
}

/// Authentication configuration for connecting to Kafka
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticationSpec {
    /// Authentication type: tls or scram-sha-512
    #[serde(rename = "type")]
    pub auth_type: AuthenticationType,
    /// Reference to a KafkaUser CR (operator resolves credentials automatically)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kafka_user_ref: Option<KafkaUserRef>,
    /// Manual TLS certificate secret reference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub certificate_and_key: Option<CertificateAndKeySecretRef>,
    /// Manual SCRAM password secret reference
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password_secret: Option<SecretKeyRef>,
    /// Username for SCRAM authentication
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthenticationType {
    Tls,
    #[serde(rename = "scram-sha-512")]
    ScramSha512,
}

/// Reference to a KafkaUser CR
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KafkaUserRef {
    /// Name of the KafkaUser CR
    pub name: String,
}

/// Reference to a TLS certificate and key in a Secret
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CertificateAndKeySecretRef {
    /// Secret name
    pub secret_name: String,
    /// Key for the certificate
    pub certificate: String,
    /// Key for the private key
    pub key: String,
}

/// Reference to a key within a Kubernetes Secret
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecretKeyRef {
    /// Secret name
    pub name: String,
    /// Key within the secret
    pub key: String,
}

/// Topic selection with include/exclude glob patterns
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TopicSelection {
    /// Glob patterns for topics to include
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// Glob patterns for topics to exclude
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

/// Consumer group selection with include/exclude regex patterns
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ConsumerGroupSelection {
    /// Regex patterns for consumer groups to include
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include: Vec<String>,
    /// Regex patterns for consumer groups to exclude
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude: Vec<String>,
}

/// Kafka TCP connection tuning passed through to kafka-backup.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct KafkaConnectionSpec {
    /// Enable TCP keepalive
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_keepalive: Option<bool>,
    /// Seconds before the first keepalive probe
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keepalive_time_secs: Option<u64>,
    /// Seconds between keepalive probes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keepalive_interval_secs: Option<u64>,
    /// Enable TCP_NODELAY
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_nodelay: Option<bool>,
    /// TCP connections to keep per broker
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connections_per_broker: Option<usize>,
}

/// Metrics HTTP server configuration for kafka-backup job pods.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct MetricsSpec {
    /// Enable the kafka-backup metrics server
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Metrics HTTP port
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// Metrics bind address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind_address: Option<String>,
    /// Metrics endpoint path
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Metrics recalculation interval in milliseconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update_interval_ms: Option<u64>,
    /// Maximum topic/partition labels emitted by the core metrics registry
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_partition_labels: Option<usize>,
}

/// Offset storage configuration for continuous backup progress.
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct OffsetStorageSpec {
    /// Offset storage backend: sqlite or memory
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<String>,
    /// Path to the local SQLite database file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub db_path: Option<String>,
    /// Remote S3 key used to sync the offset database
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s3_key: Option<String>,
    /// Remote sync interval in seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_interval_secs: Option<u64>,
}

// --- Storage types ---

/// Storage configuration for backup destination
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageSpec {
    /// Storage backend type
    #[serde(rename = "type")]
    pub storage_type: StorageType,
    /// S3-compatible storage configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub s3: Option<S3StorageSpec>,
    /// Azure Blob Storage configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub azure: Option<AzureStorageSpec>,
    /// Google Cloud Storage configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gcs: Option<GcsStorageSpec>,
    /// Filesystem storage configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<FilesystemStorageSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum StorageType {
    S3,
    Azure,
    Gcs,
    Filesystem,
}

/// S3-compatible storage configuration
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct S3StorageSpec {
    /// S3 bucket name
    pub bucket: String,
    /// AWS region
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    /// Key prefix within the bucket
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// S3-compatible endpoint URL (for MinIO, Ceph RGW, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Force path-style access (required for MinIO)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub force_path_style: Option<bool>,
    /// Allow insecure HTTP connections
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_http: Option<bool>,
    /// Secret containing AWS credentials
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials_secret: Option<SecretKeyRef>,
    /// Secret key containing AWS access key ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_key_secret: Option<SecretKeyRef>,
    /// Secret key containing AWS secret access key
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret_key_secret: Option<SecretKeyRef>,
}

/// Azure Blob Storage configuration
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct AzureStorageSpec {
    /// Azure blob container name
    pub container: String,
    /// Azure storage account name
    pub storage_account: String,
    /// Key prefix within the container
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Secret containing Azure credentials
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials_secret: Option<SecretKeyRef>,
    /// Storage account key secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account_key_secret: Option<SecretKeyRef>,
    /// SAS token secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sas_token_secret: Option<SecretKeyRef>,
    /// Service principal client secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_secret_secret: Option<SecretKeyRef>,
    /// Custom endpoint for sovereign clouds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    /// Enable Azure Workload Identity
    #[serde(skip_serializing_if = "Option::is_none")]
    pub use_workload_identity: Option<bool>,
    /// Azure AD client ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_id: Option<String>,
    /// Azure AD tenant ID
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tenant_id: Option<String>,
}

/// Google Cloud Storage configuration
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GcsStorageSpec {
    /// GCS bucket name
    pub bucket: String,
    /// Key prefix within the bucket
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Secret containing GCS service account JSON
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credentials_secret: Option<SecretKeyRef>,
    /// Path to a mounted service account JSON file
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_path: Option<String>,
}

/// Local filesystem storage configuration.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct FilesystemStorageSpec {
    /// Base path for backup data
    pub path: String,
}

// --- Pod template types ---

/// Template for customizing backup/restore pods
#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PodTemplateSpec {
    /// Pod-level overrides
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pod: Option<PodOverrides>,
    /// Container-level overrides
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ContainerOverrides>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PodOverrides {
    /// Additional metadata for the pod
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<PodMetadata>,
    /// Pod affinity rules (pass-through to k8s Affinity)
    #[schemars(schema_with = "free_form_object")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub affinity: Option<serde_json::Value>,
    /// Pod tolerations (pass-through to k8s Tolerations)
    #[schemars(schema_with = "free_form_object_array")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tolerations: Vec<serde_json::Value>,
    /// Pod security context (pass-through to k8s PodSecurityContext)
    #[schemars(schema_with = "free_form_object")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_context: Option<serde_json::Value>,
    /// Image pull secrets
    #[schemars(schema_with = "free_form_object_array")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub image_pull_secrets: Vec<serde_json::Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PodMetadata {
    /// Additional labels
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    /// Additional annotations
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContainerOverrides {
    /// Additional environment variables (pass-through to k8s EnvVar)
    #[schemars(schema_with = "free_form_object_array")]
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env: Vec<serde_json::Value>,
    /// Container security context (pass-through to k8s SecurityContext)
    #[schemars(schema_with = "free_form_object")]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security_context: Option<serde_json::Value>,
}

/// Schema helper: a nullable free-form object (type: object with no properties)
fn free_form_object(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    schemars::schema::Schema::Object(schemars::schema::SchemaObject {
        instance_type: Some(schemars::schema::InstanceType::Object.into()),
        extensions: {
            let mut map = schemars::Map::new();
            map.insert(
                "x-kubernetes-preserve-unknown-fields".to_string(),
                serde_json::Value::Bool(true),
            );
            map
        },
        ..Default::default()
    })
}

/// Schema helper: an array of free-form objects
fn free_form_object_array(_: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
    schemars::schema::Schema::Object(schemars::schema::SchemaObject {
        instance_type: Some(schemars::schema::InstanceType::Array.into()),
        array: Some(Box::new(schemars::schema::ArrayValidation {
            items: Some(schemars::schema::SingleOrVec::Single(Box::new(
                schemars::schema::Schema::Object(schemars::schema::SchemaObject {
                    instance_type: Some(schemars::schema::InstanceType::Object.into()),
                    extensions: {
                        let mut map = schemars::Map::new();
                        map.insert(
                            "x-kubernetes-preserve-unknown-fields".to_string(),
                            serde_json::Value::Bool(true),
                        );
                        map
                    },
                    ..Default::default()
                }),
            ))),
            ..Default::default()
        })),
        ..Default::default()
    })
}

// --- Status types (Strimzi convention) ---

/// Strimzi-style status condition
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Condition {
    /// Condition type (e.g., Ready, BackupComplete, Error)
    #[serde(rename = "type")]
    pub condition_type: String,
    /// Status: "True", "False", or "Unknown"
    pub status: String,
    /// Machine-readable reason
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Human-readable message
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Time of last transition
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_transition_time: Option<DateTime<Utc>>,
}

/// Backup history entry
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct BackupHistoryEntry {
    /// Unique backup ID
    pub id: String,
    /// Backup status
    pub status: BackupStatus,
    /// Start time
    pub start_time: DateTime<Utc>,
    /// Completion time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_time: Option<DateTime<Utc>>,
    /// Total size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    /// Number of topics backed up
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics_backed_up: Option<i32>,
    /// Number of partitions backed up
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partitions_backed_up: Option<i32>,
}

/// Last backup details
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LastBackupInfo {
    /// Unique backup ID
    pub id: String,
    /// Start time
    pub start_time: DateTime<Utc>,
    /// Completion time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_time: Option<DateTime<Utc>>,
    /// Backup status
    pub status: BackupStatus,
    /// Total size in bytes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<i64>,
    /// Number of topics backed up
    #[serde(skip_serializing_if = "Option::is_none")]
    pub topics_backed_up: Option<i32>,
    /// Number of partitions backed up
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partitions_backed_up: Option<i32>,
    /// Oldest record timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_timestamp: Option<DateTime<Utc>>,
    /// Newest record timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub newest_timestamp: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub enum BackupStatus {
    Running,
    Completed,
    Failed,
}

/// Restore details in status
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestoreInfo {
    /// Start time
    pub start_time: DateTime<Utc>,
    /// Completion time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completion_time: Option<DateTime<Utc>>,
    /// Restore status
    pub status: RestoreStatus,
    /// Number of topics restored
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_topics: Option<i32>,
    /// Number of partitions restored
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_partitions: Option<i32>,
    /// Total bytes restored
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restored_bytes: Option<i64>,
    /// Requested PITR timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_in_time_target: Option<DateTime<Utc>>,
    /// Actual PITR timestamp achieved
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actual_point_in_time: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub enum RestoreStatus {
    Running,
    Completed,
    Failed,
}
