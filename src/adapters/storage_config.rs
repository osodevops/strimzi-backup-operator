use serde_yaml::Value;

use crate::crd::common::{StorageSpec, StorageType};
use crate::error::{Error, Result};

/// Build the storage section of the kafka-backup config YAML
pub fn build_storage_config(storage: &StorageSpec) -> Result<Value> {
    match storage.storage_type {
        StorageType::S3 => build_s3_config(storage),
        StorageType::Azure => build_azure_config(storage),
        StorageType::Gcs => build_gcs_config(storage),
        StorageType::Filesystem => build_filesystem_config(storage),
    }
}

fn build_s3_config(storage: &StorageSpec) -> Result<Value> {
    let s3 = storage.s3.as_ref().ok_or_else(|| {
        Error::InvalidConfig("Storage type is S3 but s3 config is missing".to_string())
    })?;

    let mut config = serde_yaml::Mapping::new();
    config.insert(
        Value::String("backend".to_string()),
        Value::String("s3".to_string()),
    );
    config.insert(
        Value::String("bucket".to_string()),
        Value::String(s3.bucket.clone()),
    );

    if let Some(region) = &s3.region {
        config.insert(
            Value::String("region".to_string()),
            Value::String(region.clone()),
        );
    }

    if let Some(prefix) = &s3.prefix {
        config.insert(
            Value::String("prefix".to_string()),
            Value::String(prefix.clone()),
        );
    }

    if let Some(endpoint) = &s3.endpoint {
        config.insert(
            Value::String("endpoint".to_string()),
            Value::String(endpoint.clone()),
        );
    }

    if let Some(force_path_style) = s3.force_path_style {
        config.insert(
            Value::String("path_style".to_string()),
            Value::Bool(force_path_style),
        );
    }

    if let Some(allow_http) = s3.allow_http {
        config.insert(
            Value::String("allow_http".to_string()),
            Value::Bool(allow_http),
        );
    }

    if s3.access_key_secret.is_some() {
        config.insert(
            Value::String("access_key".to_string()),
            Value::String("${AWS_ACCESS_KEY_ID}".to_string()),
        );
    }

    if s3.secret_key_secret.is_some() {
        config.insert(
            Value::String("secret_key".to_string()),
            Value::String("${AWS_SECRET_ACCESS_KEY}".to_string()),
        );
    }

    Ok(Value::Mapping(config))
}

fn build_azure_config(storage: &StorageSpec) -> Result<Value> {
    let azure = storage.azure.as_ref().ok_or_else(|| {
        Error::InvalidConfig("Storage type is Azure but azure config is missing".to_string())
    })?;

    let mut config = serde_yaml::Mapping::new();
    config.insert(
        Value::String("backend".to_string()),
        Value::String("azure".to_string()),
    );
    config.insert(
        Value::String("container_name".to_string()),
        Value::String(azure.container.clone()),
    );
    config.insert(
        Value::String("account_name".to_string()),
        Value::String(azure.storage_account.clone()),
    );

    if let Some(prefix) = &azure.prefix {
        config.insert(
            Value::String("prefix".to_string()),
            Value::String(prefix.clone()),
        );
    }

    if azure.credentials_secret.is_some() || azure.account_key_secret.is_some() {
        config.insert(
            Value::String("account_key".to_string()),
            Value::String("${AZURE_STORAGE_KEY}".to_string()),
        );
    }

    if azure.sas_token_secret.is_some() {
        config.insert(
            Value::String("sas_token".to_string()),
            Value::String("${AZURE_STORAGE_SAS_TOKEN}".to_string()),
        );
    }

    if azure.client_secret_secret.is_some() {
        config.insert(
            Value::String("client_secret".to_string()),
            Value::String("${AZURE_CLIENT_SECRET}".to_string()),
        );
    }

    if let Some(endpoint) = &azure.endpoint {
        config.insert(
            Value::String("endpoint".to_string()),
            Value::String(endpoint.clone()),
        );
    }

    if let Some(use_workload_identity) = azure.use_workload_identity {
        config.insert(
            Value::String("use_workload_identity".to_string()),
            Value::Bool(use_workload_identity),
        );
    }

    if let Some(client_id) = &azure.client_id {
        config.insert(
            Value::String("client_id".to_string()),
            Value::String(client_id.clone()),
        );
    }

    if let Some(tenant_id) = &azure.tenant_id {
        config.insert(
            Value::String("tenant_id".to_string()),
            Value::String(tenant_id.clone()),
        );
    }

    Ok(Value::Mapping(config))
}

fn build_gcs_config(storage: &StorageSpec) -> Result<Value> {
    let gcs = storage.gcs.as_ref().ok_or_else(|| {
        Error::InvalidConfig("Storage type is GCS but gcs config is missing".to_string())
    })?;

    let mut config = serde_yaml::Mapping::new();
    config.insert(
        Value::String("backend".to_string()),
        Value::String("gcs".to_string()),
    );
    config.insert(
        Value::String("bucket".to_string()),
        Value::String(gcs.bucket.clone()),
    );

    if let Some(prefix) = &gcs.prefix {
        config.insert(
            Value::String("prefix".to_string()),
            Value::String(prefix.clone()),
        );
    }

    if gcs.credentials_secret.is_some() {
        config.insert(
            Value::String("service_account_path".to_string()),
            Value::String("/credentials/credentials".to_string()),
        );
    } else if let Some(service_account_path) = &gcs.service_account_path {
        config.insert(
            Value::String("service_account_path".to_string()),
            Value::String(service_account_path.clone()),
        );
    }

    Ok(Value::Mapping(config))
}

fn build_filesystem_config(storage: &StorageSpec) -> Result<Value> {
    let filesystem = storage.filesystem.as_ref().ok_or_else(|| {
        Error::InvalidConfig(
            "Storage type is Filesystem but filesystem config is missing".to_string(),
        )
    })?;

    let mut config = serde_yaml::Mapping::new();
    config.insert(
        Value::String("backend".to_string()),
        Value::String("filesystem".to_string()),
    );
    config.insert(
        Value::String("path".to_string()),
        Value::String(filesystem.path.clone()),
    );

    Ok(Value::Mapping(config))
}

/// Get the credentials secret name from a StorageSpec, if any
pub fn get_storage_credentials_secret(storage: &StorageSpec) -> Option<(String, String)> {
    match storage.storage_type {
        StorageType::S3 => storage
            .s3
            .as_ref()
            .and_then(|s| s.credentials_secret.as_ref())
            .map(|s| (s.name.clone(), s.key.clone())),
        StorageType::Azure => storage
            .azure
            .as_ref()
            .and_then(|a| a.credentials_secret.as_ref())
            .map(|s| (s.name.clone(), s.key.clone())),
        StorageType::Gcs => storage
            .gcs
            .as_ref()
            .and_then(|g| g.credentials_secret.as_ref())
            .map(|s| (s.name.clone(), s.key.clone())),
        StorageType::Filesystem => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crd::common::{S3StorageSpec, SecretKeyRef};

    #[test]
    fn test_build_s3_config() {
        let storage = StorageSpec {
            storage_type: StorageType::S3,
            s3: Some(S3StorageSpec {
                bucket: "my-bucket".to_string(),
                region: Some("us-east-1".to_string()),
                prefix: Some("backups/".to_string()),
                endpoint: None,
                force_path_style: None,
                allow_http: None,
                credentials_secret: Some(SecretKeyRef {
                    name: "aws-creds".to_string(),
                    key: "credentials".to_string(),
                }),
                access_key_secret: None,
                secret_key_secret: None,
            }),
            azure: None,
            gcs: None,
            filesystem: None,
        };

        let config = build_storage_config(&storage).unwrap();
        let mapping = config.as_mapping().unwrap();
        assert_eq!(
            mapping.get(Value::String("backend".to_string())),
            Some(&Value::String("s3".to_string()))
        );
        assert_eq!(
            mapping.get(Value::String("bucket".to_string())),
            Some(&Value::String("my-bucket".to_string()))
        );
        assert_eq!(
            mapping.get(Value::String("region".to_string())),
            Some(&Value::String("us-east-1".to_string()))
        );
    }
}
