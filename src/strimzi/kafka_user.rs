use kube::{
    api::{Api, DynamicObject, GroupVersionKind},
    Client,
};
use tracing::{debug, info};

use crate::crd::common::{AuthenticationSpec, AuthenticationType};
use crate::error::{Error, Result};

/// Resolved authentication credentials
#[derive(Clone, Debug)]
pub enum ResolvedAuth {
    /// TLS client certificate authentication
    Tls {
        /// Secret name containing user.crt, user.key, user.p12
        secret_name: String,
    },
    /// SCRAM-SHA-512 authentication
    ScramSha512 {
        /// Username
        username: String,
        /// Secret name containing the password
        secret_name: String,
        /// Key within the secret that holds the password
        password_key: String,
    },
    /// No authentication
    None,
}

/// Resolve authentication credentials from a KafkaBackup/KafkaRestore spec
pub async fn resolve_auth(
    client: &Client,
    auth_spec: Option<&AuthenticationSpec>,
    namespace: &str,
) -> Result<ResolvedAuth> {
    let Some(auth) = auth_spec else {
        return Ok(ResolvedAuth::None);
    };

    match auth.auth_type {
        AuthenticationType::Tls => resolve_tls_auth(client, auth, namespace).await,
        AuthenticationType::ScramSha512 => resolve_scram_auth(client, auth, namespace).await,
    }
}

/// Resolve TLS authentication credentials
async fn resolve_tls_auth(
    client: &Client,
    auth: &AuthenticationSpec,
    namespace: &str,
) -> Result<ResolvedAuth> {
    // If kafkaUserRef is set, resolve the user's secret
    if let Some(user_ref) = &auth.kafka_user_ref {
        let secret_name = resolve_kafka_user_secret(client, &user_ref.name, namespace).await?;
        info!(%secret_name, "Resolved TLS credentials from KafkaUser");
        return Ok(ResolvedAuth::Tls { secret_name });
    }

    // Otherwise, use the manual certificate reference
    if let Some(cert_ref) = &auth.certificate_and_key {
        return Ok(ResolvedAuth::Tls {
            secret_name: cert_ref.secret_name.clone(),
        });
    }

    Err(Error::InvalidConfig(
        "TLS authentication requires either kafkaUserRef or certificateAndKey".to_string(),
    ))
}

/// Resolve SCRAM-SHA-512 authentication credentials
async fn resolve_scram_auth(
    client: &Client,
    auth: &AuthenticationSpec,
    namespace: &str,
) -> Result<ResolvedAuth> {
    // If kafkaUserRef is set, resolve the user's secret
    if let Some(user_ref) = &auth.kafka_user_ref {
        let secret_name = resolve_kafka_user_secret(client, &user_ref.name, namespace).await?;
        info!(%secret_name, "Resolved SCRAM credentials from KafkaUser");
        return Ok(ResolvedAuth::ScramSha512 {
            username: user_ref.name.clone(),
            secret_name,
            password_key: "password".to_string(),
        });
    }

    // Otherwise, use manual references
    let username = auth.username.clone().ok_or_else(|| {
        Error::InvalidConfig("SCRAM authentication requires username".to_string())
    })?;

    let password_secret = auth.password_secret.as_ref().ok_or_else(|| {
        Error::InvalidConfig(
            "SCRAM authentication requires either kafkaUserRef or passwordSecret".to_string(),
        )
    })?;

    Ok(ResolvedAuth::ScramSha512 {
        username,
        secret_name: password_secret.name.clone(),
        password_key: password_secret.key.clone(),
    })
}

/// Resolve the secret name created by the Strimzi User Operator for a KafkaUser
async fn resolve_kafka_user_secret(
    client: &Client,
    user_name: &str,
    namespace: &str,
) -> Result<String> {
    info!(%user_name, %namespace, "Resolving KafkaUser secret");

    let api: Api<DynamicObject> = Api::namespaced_with(
        client.clone(),
        namespace,
        &kube::api::ApiResource::from_gvk(&GroupVersionKind::gvk(
            "kafka.strimzi.io",
            "v1beta2",
            "KafkaUser",
        )),
    );

    let user = api.get(user_name).await.map_err(|e| match &e {
        kube::Error::Api(ae) if ae.code == 404 => Error::KafkaUserNotFound {
            name: user_name.to_string(),
            namespace: namespace.to_string(),
        },
        _ => Error::Kube(e),
    })?;

    // Check if status.secret is set
    if let Some(status) = user.data.get("status") {
        if let Some(secret) = status.get("secret").and_then(|s| s.as_str()) {
            return Ok(secret.to_string());
        }
    }

    // Fall back to conventional name (same as user name)
    debug!(%user_name, "Using conventional secret name for KafkaUser");
    Ok(user_name.to_string())
}
