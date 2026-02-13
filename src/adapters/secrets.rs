use k8s_openapi::api::core::v1::Secret;
use kube::{Api, Client};

use crate::error::{Error, Result};

/// Fetch a Kubernetes Secret by name and namespace
pub async fn get_secret(client: &Client, name: &str, namespace: &str) -> Result<Secret> {
    let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);
    secrets.get(name).await.map_err(|e| match &e {
        kube::Error::Api(ae) if ae.code == 404 => Error::SecretNotFound {
            name: name.to_string(),
            namespace: namespace.to_string(),
        },
        _ => Error::Kube(e),
    })
}

/// Extract a string value from a Secret's data field
pub fn extract_secret_data(secret: &Secret, key: &str) -> Result<String> {
    let name = secret.metadata.name.as_deref().unwrap_or("unknown");

    let data = secret
        .data
        .as_ref()
        .ok_or_else(|| Error::SecretKeyMissing {
            name: name.to_string(),
            key: key.to_string(),
        })?;

    let bytes = data.get(key).ok_or_else(|| Error::SecretKeyMissing {
        name: name.to_string(),
        key: key.to_string(),
    })?;

    String::from_utf8(bytes.0.clone()).map_err(|_| Error::SecretKeyMissing {
        name: name.to_string(),
        key: key.to_string(),
    })
}

/// Extract raw bytes from a Secret's data field
pub fn extract_secret_bytes(secret: &Secret, key: &str) -> Result<Vec<u8>> {
    let name = secret.metadata.name.as_deref().unwrap_or("unknown");

    let data = secret
        .data
        .as_ref()
        .ok_or_else(|| Error::SecretKeyMissing {
            name: name.to_string(),
            key: key.to_string(),
        })?;

    let bytes = data.get(key).ok_or_else(|| Error::SecretKeyMissing {
        name: name.to_string(),
        key: key.to_string(),
    })?;

    Ok(bytes.0.clone())
}
