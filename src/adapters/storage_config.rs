use serde_yaml::Value;

use crate::crd::common::{StorageSpec, StorageType};
use crate::error::{Error, Result};

/// Build the storage section of the kafka-backup config YAML
pub fn build_storage_config(storage: &StorageSpec) -> Result<Value> {
    match storage.storage_type {
        StorageType::S3 => build_s3_config(storage),
        StorageType::Azure => build_azure_config(storage),
        StorageType::Gcs => build_gcs_config(storage),
    }
}

fn build_s3_config(storage: &StorageSpec) -> Result<Value> {
    let s3 = storage.s3.as_ref().ok_or_else(|| {
        Error::InvalidConfig("Storage type is S3 but s3 config is missing".to_string())
    })?;

    let mut config = serde_yaml::Mapping::new();
    config.insert(
        Value::String("type".to_string()),
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
            Value::String("force_path_style".to_string()),
            Value::Bool(force_path_style),
        );
    }

    // Credentials are mounted as environment variables or files by the Job
    // The path is set to /credentials/ in the Job spec
    if s3.credentials_secret.is_some() {
        config.insert(
            Value::String("credentials_file".to_string()),
            Value::String("/credentials/credentials".to_string()),
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
        Value::String("type".to_string()),
        Value::String("azure".to_string()),
    );
    config.insert(
        Value::String("container".to_string()),
        Value::String(azure.container.clone()),
    );
    config.insert(
        Value::String("storage_account".to_string()),
        Value::String(azure.storage_account.clone()),
    );

    if let Some(prefix) = &azure.prefix {
        config.insert(
            Value::String("prefix".to_string()),
            Value::String(prefix.clone()),
        );
    }

    if azure.credentials_secret.is_some() {
        config.insert(
            Value::String("credentials_file".to_string()),
            Value::String("/credentials/credentials".to_string()),
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
        Value::String("type".to_string()),
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
            Value::String("credentials_file".to_string()),
            Value::String("/credentials/credentials".to_string()),
        );
    }

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
                credentials_secret: Some(SecretKeyRef {
                    name: "aws-creds".to_string(),
                    key: "credentials".to_string(),
                }),
            }),
            azure: None,
            gcs: None,
        };

        let config = build_storage_config(&storage).unwrap();
        let mapping = config.as_mapping().unwrap();
        assert_eq!(
            mapping.get(&Value::String("type".to_string())),
            Some(&Value::String("s3".to_string()))
        );
        assert_eq!(
            mapping.get(&Value::String("bucket".to_string())),
            Some(&Value::String("my-bucket".to_string()))
        );
        assert_eq!(
            mapping.get(&Value::String("region".to_string())),
            Some(&Value::String("us-east-1".to_string()))
        );
    }
}
