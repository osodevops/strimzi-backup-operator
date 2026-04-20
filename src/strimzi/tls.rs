use k8s_openapi::api::core::v1::Secret;
use kube::{Api, Client};
use tracing::{debug, info};

use crate::crd::common::SecretKeyRef;
use crate::error::{Error, Result};

/// Default key for the cluster CA certificate inside a Strimzi CA secret.
pub const DEFAULT_CA_KEY: &str = "ca.crt";

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

/// Resolve the cluster CA certificate. When `ca_override` is provided, the
/// secret name and key come from the override; otherwise the operator falls
/// back to the Strimzi convention (`{cluster}-cluster-ca-cert` / `ca.crt`).
pub async fn resolve_cluster_ca(
    client: &Client,
    cluster_name: &str,
    ca_override: Option<&SecretKeyRef>,
    namespace: &str,
) -> Result<ResolvedTlsCerts> {
    let (secret_name, ca_key) = ca_secret_ref(cluster_name, ca_override);
    info!(%secret_name, key = %ca_key, %namespace, "Resolving cluster CA certificate");

    let secrets: Api<Secret> = Api::namespaced(client.clone(), namespace);

    let ca_secret = secrets.get(&secret_name).await.map_err(|e| match &e {
        kube::Error::Api(ae) if ae.code == 404 => Error::SecretNotFound {
            name: secret_name.clone(),
            namespace: namespace.to_string(),
        },
        _ => Error::Kube(e),
    })?;

    let cluster_ca_cert = extract_secret_string(&ca_secret, &ca_key, &secret_name)?;

    // Try to get client CA (may not exist). Only looked up when the caller
    // didn't override the CA secret — an override implies a non-Strimzi layout.
    let client_ca_cert = if ca_override.is_some() {
        None
    } else {
        let client_ca_name = format!("{cluster_name}-clients-ca-cert");
        match secrets.get(&client_ca_name).await {
            Ok(client_secret) => {
                extract_secret_string(&client_secret, DEFAULT_CA_KEY, &client_ca_name).ok()
            }
            Err(_) => {
                debug!(%client_ca_name, "Client CA secret not found (optional)");
                None
            }
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

/// Resolve the CA secret name and key to read, honouring an optional override.
pub fn ca_secret_ref(cluster_name: &str, ca_override: Option<&SecretKeyRef>) -> (String, String) {
    match ca_override {
        Some(r) => (r.name.clone(), r.key.clone()),
        None => (cluster_ca_secret_name(cluster_name), DEFAULT_CA_KEY.to_string()),
    }
}

/// Get the conventional Strimzi secret names for volume mounts
pub fn cluster_ca_secret_name(cluster_name: &str) -> String {
    format!("{cluster_name}-cluster-ca-cert")
}

pub fn clients_ca_secret_name(cluster_name: &str) -> String {
    format!("{cluster_name}-clients-ca-cert")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ca_secret_ref_defaults_to_strimzi_convention() {
        let (name, key) = ca_secret_ref("my-cluster", None);
        assert_eq!(name, "my-cluster-cluster-ca-cert");
        assert_eq!(key, "ca.crt");
    }

    #[test]
    fn ca_secret_ref_honours_override() {
        let override_ref = SecretKeyRef {
            name: "custom-ca".to_string(),
            key: "tls.crt".to_string(),
        };
        let (name, key) = ca_secret_ref("my-cluster", Some(&override_ref));
        assert_eq!(name, "custom-ca");
        assert_eq!(key, "tls.crt");
    }
}
