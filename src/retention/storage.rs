use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures::StreamExt;
use object_store::aws::AmazonS3Builder;
use object_store::azure::MicrosoftAzureBuilder;
use object_store::gcp::GoogleCloudStorageBuilder;
use object_store::local::LocalFileSystem;
use object_store::path::Path;
use object_store::ObjectStore;
use serde::Deserialize;
use tracing::{debug, info, warn};

use crate::adapters::secrets::{extract_secret_data, get_secret};
use crate::crd::common::{BackupHistoryEntry, BackupStatus, StorageSpec, StorageType};
use crate::error::{Error, Result};

struct BackupObjectStore {
    store: Arc<dyn ObjectStore>,
    prefix: Option<String>,
}

impl BackupObjectStore {
    fn new(store: Arc<dyn ObjectStore>, prefix: Option<String>) -> Self {
        Self {
            store,
            prefix: normalize_optional_prefix(prefix),
        }
    }

    fn full_path(&self, key: &str) -> Path {
        Path::from(join_key(self.prefix.as_deref(), key))
    }

    fn strip_prefix(&self, key: &str) -> String {
        match self.prefix.as_deref() {
            Some(prefix) => key
                .strip_prefix(&format!("{prefix}/"))
                .unwrap_or(key)
                .to_string(),
            None => key.to_string(),
        }
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let path = self.full_path(key);
        let response = self.store.get(&path).await.map_err(storage_error)?;
        let bytes = response.bytes().await.map_err(storage_error)?;
        Ok(bytes.to_vec())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<String>> {
        let path = self.full_path(prefix);
        let mut stream = self.store.list(Some(&path));
        let mut keys = Vec::new();

        while let Some(item) = stream.next().await {
            let metadata = item.map_err(storage_error)?;
            keys.push(self.strip_prefix(metadata.location.as_ref()));
        }

        keys.sort();
        Ok(keys)
    }

    async fn delete(&self, key: &str) -> Result<()> {
        let path = self.full_path(key);
        match self.store.delete(&path).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(e) => Err(storage_error(e)),
        }
    }
}

#[derive(Debug, Deserialize)]
struct StoredBackupManifest {
    backup_id: String,
    created_at: i64,
    #[serde(default)]
    topics: Vec<StoredTopic>,
}

#[derive(Debug, Deserialize)]
struct StoredTopic {
    #[serde(default)]
    partitions: Vec<StoredPartition>,
}

#[derive(Debug, Deserialize)]
struct StoredPartition {
    #[serde(default)]
    segments: Vec<StoredSegment>,
}

#[derive(Debug, Deserialize)]
struct StoredSegment {
    #[serde(default)]
    compressed_size: u64,
}

pub async fn discover_backup_history(
    client: &kube::Client,
    namespace: &str,
    storage: &StorageSpec,
    owner_name: &str,
) -> Result<Vec<BackupHistoryEntry>> {
    let store = build_store(client, namespace, storage).await?;
    let keys = store.list("").await?;
    let mut history = Vec::new();

    for key in keys.iter().filter(|k| k.ends_with("/manifest.json")) {
        let backup_id = key.trim_end_matches("/manifest.json");
        if !backup_id_belongs_to_cr(backup_id, owner_name) {
            continue;
        }

        let bytes = match store.get(key).await {
            Ok(bytes) => bytes,
            Err(e) => {
                warn!(%key, error = %e, "Failed to read backup manifest during retention discovery");
                continue;
            }
        };

        let manifest: StoredBackupManifest = match serde_json::from_slice(&bytes) {
            Ok(manifest) => manifest,
            Err(e) => {
                warn!(%key, error = %e, "Failed to parse backup manifest during retention discovery");
                continue;
            }
        };

        history.push(manifest_to_history_entry(manifest));
    }

    Ok(history)
}

pub async fn prune_backup_ids(
    client: &kube::Client,
    namespace: &str,
    storage: &StorageSpec,
    backup_ids: &[String],
) -> Result<Vec<String>> {
    if backup_ids.is_empty() {
        return Ok(Vec::new());
    }

    let store = build_store(client, namespace, storage).await?;
    let mut pruned = Vec::new();

    for backup_id in backup_ids {
        let prefix = format!("{}/", backup_id.trim_end_matches('/'));
        let keys = store.list(&prefix).await?;

        if keys.is_empty() {
            debug!(%backup_id, "No storage objects found for retained backup id");
            pruned.push(backup_id.clone());
            continue;
        }

        for key in keys.iter().filter(|key| key.starts_with(&prefix)) {
            store.delete(key).await?;
        }

        info!(
            %backup_id,
            deleted_objects = keys.len(),
            "Pruned expired backup from storage"
        );
        pruned.push(backup_id.clone());
    }

    Ok(pruned)
}

pub fn backup_id_belongs_to_cr(backup_id: &str, owner_name: &str) -> bool {
    backup_id == owner_name || backup_id.starts_with(&format!("{owner_name}-"))
}

async fn build_store(
    client: &kube::Client,
    namespace: &str,
    storage: &StorageSpec,
) -> Result<BackupObjectStore> {
    match storage.storage_type {
        StorageType::S3 => build_s3_store(client, namespace, storage).await,
        StorageType::Azure => build_azure_store(client, namespace, storage).await,
        StorageType::Gcs => build_gcs_store(client, namespace, storage).await,
        StorageType::Filesystem => build_filesystem_store(storage),
    }
}

async fn build_s3_store(
    client: &kube::Client,
    namespace: &str,
    storage: &StorageSpec,
) -> Result<BackupObjectStore> {
    let s3 = storage.s3.as_ref().ok_or_else(|| {
        Error::InvalidConfig("Storage type is S3 but s3 config is missing".to_string())
    })?;

    let mut builder = AmazonS3Builder::from_env().with_bucket_name(&s3.bucket);

    if let Some(region) = &s3.region {
        builder = builder.with_region(region);
    }
    if let Some(endpoint) = &s3.endpoint {
        builder = builder.with_endpoint(endpoint);
        builder = builder.with_virtual_hosted_style_request(false);
    }
    if s3.force_path_style.unwrap_or(false) {
        builder = builder.with_virtual_hosted_style_request(false);
    }
    if s3.allow_http.unwrap_or(false) {
        builder = builder.with_allow_http(true);
    }

    let mut access_key = None;
    let mut secret_key = None;
    let mut token = None;

    if let Some(secret_ref) = &s3.credentials_secret {
        let secret = get_secret(client, &secret_ref.name, namespace).await?;
        let credentials = extract_secret_data(&secret, &secret_ref.key)?;
        let parsed = parse_aws_shared_credentials(&credentials);
        access_key = parsed.access_key_id;
        secret_key = parsed.secret_access_key;
        token = parsed.session_token;
    }

    if let Some(secret_ref) = &s3.access_key_secret {
        let secret = get_secret(client, &secret_ref.name, namespace).await?;
        access_key = Some(extract_secret_data(&secret, &secret_ref.key)?);
    }
    if let Some(secret_ref) = &s3.secret_key_secret {
        let secret = get_secret(client, &secret_ref.name, namespace).await?;
        secret_key = Some(extract_secret_data(&secret, &secret_ref.key)?);
    }

    if let Some(access_key) = access_key {
        builder = builder.with_access_key_id(access_key);
    }
    if let Some(secret_key) = secret_key {
        builder = builder.with_secret_access_key(secret_key);
    }
    if let Some(token) = token {
        builder = builder.with_token(token);
    }

    let store = builder.build().map_err(storage_error)?;
    Ok(BackupObjectStore::new(Arc::new(store), s3.prefix.clone()))
}

async fn build_azure_store(
    client: &kube::Client,
    namespace: &str,
    storage: &StorageSpec,
) -> Result<BackupObjectStore> {
    let azure = storage.azure.as_ref().ok_or_else(|| {
        Error::InvalidConfig("Storage type is Azure but azure config is missing".to_string())
    })?;

    let mut builder = MicrosoftAzureBuilder::new()
        .with_account(&azure.storage_account)
        .with_container_name(&azure.container);

    if let Some(endpoint) = &azure.endpoint {
        builder = builder.with_endpoint(endpoint.clone());
        if endpoint.starts_with("http://") {
            builder = builder.with_allow_http(true);
        }
    }

    let account_key_ref = azure
        .account_key_secret
        .as_ref()
        .or(azure.credentials_secret.as_ref());
    if let Some(secret_ref) = account_key_ref {
        let secret = get_secret(client, &secret_ref.name, namespace).await?;
        builder = builder.with_access_key(extract_secret_data(&secret, &secret_ref.key)?);
    }

    if let Some(secret_ref) = &azure.sas_token_secret {
        let secret = get_secret(client, &secret_ref.name, namespace).await?;
        let sas_token = extract_secret_data(&secret, &secret_ref.key)?;
        let pairs = sas_token_pairs(&sas_token);
        builder = builder.with_sas_authorization(pairs);
    }

    if let Some(client_id) = &azure.client_id {
        builder = builder.with_client_id(client_id);
    }
    if let Some(tenant_id) = &azure.tenant_id {
        builder = builder.with_tenant_id(tenant_id);
    }
    if let Some(secret_ref) = &azure.client_secret_secret {
        let secret = get_secret(client, &secret_ref.name, namespace).await?;
        builder = builder.with_client_secret(extract_secret_data(&secret, &secret_ref.key)?);
    }
    if azure.use_workload_identity.unwrap_or(false) {
        if let Ok(token_file) = std::env::var("AZURE_FEDERATED_TOKEN_FILE") {
            builder = builder.with_federated_token_file(token_file);
        }
        builder = builder.with_use_azure_cli(false);
    }

    let store = builder.build().map_err(storage_error)?;
    Ok(BackupObjectStore::new(
        Arc::new(store),
        azure.prefix.clone(),
    ))
}

async fn build_gcs_store(
    client: &kube::Client,
    namespace: &str,
    storage: &StorageSpec,
) -> Result<BackupObjectStore> {
    let gcs = storage.gcs.as_ref().ok_or_else(|| {
        Error::InvalidConfig("Storage type is GCS but gcs config is missing".to_string())
    })?;

    let mut builder = GoogleCloudStorageBuilder::new().with_bucket_name(&gcs.bucket);

    if let Some(secret_ref) = &gcs.credentials_secret {
        let secret = get_secret(client, &secret_ref.name, namespace).await?;
        builder = builder.with_service_account_key(extract_secret_data(&secret, &secret_ref.key)?);
    } else if let Some(path) = &gcs.service_account_path {
        builder = builder.with_service_account_path(path);
    }

    let store = builder.build().map_err(storage_error)?;
    Ok(BackupObjectStore::new(Arc::new(store), gcs.prefix.clone()))
}

fn build_filesystem_store(storage: &StorageSpec) -> Result<BackupObjectStore> {
    let filesystem = storage.filesystem.as_ref().ok_or_else(|| {
        Error::InvalidConfig(
            "Storage type is Filesystem but filesystem config is missing".to_string(),
        )
    })?;
    let store = LocalFileSystem::new_with_prefix(&filesystem.path).map_err(storage_error)?;
    Ok(BackupObjectStore::new(Arc::new(store), None))
}

fn manifest_to_history_entry(manifest: StoredBackupManifest) -> BackupHistoryEntry {
    let start_time =
        DateTime::<Utc>::from_timestamp_millis(manifest.created_at).unwrap_or_else(Utc::now);
    let topics_backed_up = i32::try_from(manifest.topics.len()).ok();
    let partitions_backed_up = manifest
        .topics
        .iter()
        .map(|topic| topic.partitions.len())
        .sum::<usize>();
    let size_bytes = manifest
        .topics
        .iter()
        .flat_map(|topic| &topic.partitions)
        .flat_map(|partition| &partition.segments)
        .map(|segment| segment.compressed_size)
        .sum::<u64>();

    BackupHistoryEntry {
        id: manifest.backup_id,
        status: BackupStatus::Completed,
        start_time,
        completion_time: None,
        size_bytes: i64::try_from(size_bytes).ok(),
        topics_backed_up,
        partitions_backed_up: i32::try_from(partitions_backed_up).ok(),
    }
}

#[derive(Default)]
struct AwsSharedCredentials {
    access_key_id: Option<String>,
    secret_access_key: Option<String>,
    session_token: Option<String>,
}

fn parse_aws_shared_credentials(input: &str) -> AwsSharedCredentials {
    let mut credentials = AwsSharedCredentials::default();

    for line in input.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with('[') {
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = trim_credential_value(value);

        match key {
            "aws_access_key_id" | "access_key" | "access_key_id" => {
                credentials.access_key_id = Some(value);
            }
            "aws_secret_access_key" | "secret_key" | "secret_access_key" => {
                credentials.secret_access_key = Some(value);
            }
            "aws_session_token" | "session_token" | "token" => {
                credentials.session_token = Some(value);
            }
            _ => {}
        }
    }

    credentials
}

fn trim_credential_value(value: &str) -> String {
    value
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .to_string()
}

fn sas_token_pairs(token: &str) -> Vec<(String, String)> {
    token
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn normalize_optional_prefix(prefix: Option<String>) -> Option<String> {
    prefix
        .map(|prefix| prefix.trim_matches('/').to_string())
        .filter(|prefix| !prefix.is_empty())
}

fn join_key(prefix: Option<&str>, key: &str) -> String {
    let key = key.trim_start_matches('/');
    match (prefix, key.is_empty()) {
        (Some(prefix), false) => format!("{prefix}/{key}"),
        (Some(prefix), true) => prefix.to_string(),
        (None, _) => key.to_string(),
    }
}

fn storage_error(error: object_store::Error) -> Error {
    Error::Storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn backup_id_filter_matches_operator_owned_names() {
        assert!(backup_id_belongs_to_cr("daily-backup", "daily-backup"));
        assert!(backup_id_belongs_to_cr(
            "daily-backup-scheduled-28819192",
            "daily-backup"
        ));
        assert!(backup_id_belongs_to_cr(
            "daily-backup-20260428-010203",
            "daily-backup"
        ));
        assert!(!backup_id_belongs_to_cr("daily-backup2-1", "daily-backup"));
    }

    #[test]
    fn aws_shared_credentials_are_parsed() {
        let credentials = parse_aws_shared_credentials(
            r#"
            [default]
            aws_access_key_id = "access"
            aws_secret_access_key = 'secret'
            aws_session_token = token
            "#,
        );

        assert_eq!(credentials.access_key_id.as_deref(), Some("access"));
        assert_eq!(credentials.secret_access_key.as_deref(), Some("secret"));
        assert_eq!(credentials.session_token.as_deref(), Some("token"));
    }

    #[test]
    fn manifest_entry_uses_core_layout_metadata() {
        let manifest = StoredBackupManifest {
            backup_id: "daily-backup-1".to_string(),
            created_at: 1_777_334_400_000,
            topics: vec![StoredTopic {
                partitions: vec![StoredPartition {
                    segments: vec![
                        StoredSegment {
                            compressed_size: 100,
                        },
                        StoredSegment {
                            compressed_size: 200,
                        },
                    ],
                }],
            }],
        };

        let entry = manifest_to_history_entry(manifest);
        assert_eq!(entry.id, "daily-backup-1");
        assert_eq!(
            entry.start_time,
            Utc.with_ymd_and_hms(2026, 4, 28, 0, 0, 0).unwrap()
        );
        assert_eq!(entry.size_bytes, Some(300));
        assert_eq!(entry.topics_backed_up, Some(1));
        assert_eq!(entry.partitions_backed_up, Some(1));
    }

    #[tokio::test]
    async fn local_store_lists_and_deletes_backup_prefix() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let backup_dir = temp_dir.path().join("daily-backup-1");
        std::fs::create_dir_all(backup_dir.join("topics/orders/partition=0")).unwrap();
        std::fs::write(backup_dir.join("manifest.json"), "{}").unwrap();
        std::fs::write(
            backup_dir.join("topics/orders/partition=0/segment-000.bin.zst"),
            b"segment",
        )
        .unwrap();

        let store = BackupObjectStore::new(
            Arc::new(LocalFileSystem::new_with_prefix(temp_dir.path()).unwrap()),
            None,
        );

        let keys = store.list("daily-backup-1/").await.unwrap();
        assert_eq!(keys.len(), 2);

        for key in keys {
            store.delete(&key).await.unwrap();
        }

        assert!(store.list("daily-backup-1/").await.unwrap().is_empty());
    }
}
