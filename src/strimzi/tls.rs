use k8s_openapi::api::core::v1::Secret;
use kube::{Api, Client};
use tracing::{debug, info};

use crate::error::{Error, Result};

/// Resolved TLS certificates from Strimzi secrets
#[derive(Clone, Debug)]
pub struct ResolvedTlsCerts {
    /// Cluster CA certificate (PEM)
    pub cluster_ca_cert: String,
    /// Whether client CA is available
    pub has_client_ca: bool,
    /// Client CA certificate (PEM), if available
    pub client_ca_cert: Option<String>,
}

/// Resolve Strimzi cluster CA certificate from the conventional secret name
pub async fn resolve_cluster_ca(
    client: &Client,
    cluster_name: &str,
    namespace: &str,
) -> Result<ResolvedTlsCerts> {
    let secret_name = format!("{cluster_name}-cluster-ca-cert");
    info!(%secret_name, %namespace, "Resolving Strimzi cluster CA certificate");

    let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);

    let ca_secret = secrets.get(&secret_name).await.map_err(|e| match &e {
        kube::Error::Api(ae) if ae.code == 404 => Error::SecretNotFound {
            name: secret_name.clone(),
            namespace: namespace.to_string(),
        },
        _ => Error::Kube(e),
    })?;

    let cluster_ca_cert = extract_secret_string(&ca_secret, "ca.crt", &secret_name)?;

    // Try to get client CA (may not exist)
    let client_ca_name = format!("{cluster_name}-clients-ca-cert");
    let client_ca_cert = match secrets.get(&client_ca_name).await {
        Ok(client_secret) => extract_secret_string(&client_secret, "ca.crt", &client_ca_name).ok(),
        Err(_) => {
            debug!(%client_ca_name, "Client CA secret not found (optional)");
            None
        }
    };

    Ok(ResolvedTlsCerts {
        cluster_ca_cert,
        has_client_ca: client_ca_cert.is_some(),
        client_ca_cert,
    })
}

/// Extract a string value from a Kubernetes secret
pub fn extract_secret_string(secret: &Secret, key: &str, secret_name: &str) -> Result<String> {
    let data = secret
        .data
        .as_ref()
        .ok_or_else(|| Error::SecretKeyMissing {
            name: secret_name.to_string(),
            key: key.to_string(),
        })?;

    let bytes = data.get(key).ok_or_else(|| Error::SecretKeyMissing {
        name: secret_name.to_string(),
        key: key.to_string(),
    })?;

    String::from_utf8(bytes.0.clone()).map_err(|_| Error::SecretKeyMissing {
        name: secret_name.to_string(),
        key: key.to_string(),
    })
}

/// Get the conventional Strimzi secret names for volume mounts
pub fn cluster_ca_secret_name(cluster_name: &str) -> String {
    format!("{cluster_name}-cluster-ca-cert")
}

pub fn clients_ca_secret_name(cluster_name: &str) -> String {
    format!("{cluster_name}-clients-ca-cert")
}
