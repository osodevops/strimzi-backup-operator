use kube::{
    api::{Api, DynamicObject, GroupVersionKind},
    Client, ResourceExt,
};
use tracing::{debug, info};

use crate::crd::common::StrimziClusterRef;
use crate::error::{Error, Result};

/// Resolved information from a Strimzi Kafka CR
#[derive(Clone, Debug)]
pub struct ResolvedKafkaCluster {
    /// Kafka CR name
    pub name: String,
    /// Kafka CR namespace
    pub namespace: String,
    /// Bootstrap server addresses
    pub bootstrap_servers: String,
    /// Number of broker replicas
    pub replicas: i32,
    /// Whether TLS is enabled on the listener
    pub tls_enabled: bool,
    /// Listener name used for bootstrap
    pub listener_name: String,
}

/// Resolve a Strimzi Kafka CR reference to get cluster connection details
pub async fn resolve_kafka_cluster(
    client: &Client,
    cluster_ref: &StrimziClusterRef,
    default_namespace: &str,
) -> Result<ResolvedKafkaCluster> {
    let namespace = cluster_ref
        .namespace
        .as_deref()
        .unwrap_or(default_namespace);
    let name = &cluster_ref.name;

    info!(%name, %namespace, "Resolving Strimzi Kafka cluster");

    let api: Api<DynamicObject> = Api::namespaced_with(
        client.clone(),
        namespace,
        &kube::api::ApiResource::from_gvk(&GroupVersionKind::gvk(
            "kafka.strimzi.io",
            "v1beta2",
            "Kafka",
        )),
    );

    let kafka = api.get(name).await.map_err(|e| match &e {
        kube::Error::Api(ae) if ae.code == 404 => Error::StrimziClusterNotFound {
            name: name.clone(),
            namespace: namespace.to_string(),
        },
        _ => Error::Kube(e),
    })?;

    let bootstrap_servers = extract_bootstrap_servers(&kafka, namespace)?;
    let replicas = extract_replicas(&kafka);
    let (tls_enabled, listener_name) = extract_listener_info(&kafka);

    let resolved = ResolvedKafkaCluster {
        name: name.clone(),
        namespace: namespace.to_string(),
        bootstrap_servers,
        replicas,
        tls_enabled,
        listener_name,
    };

    debug!(?resolved, "Resolved Kafka cluster");
    Ok(resolved)
}

/// Extract bootstrap servers from the Kafka CR status
fn extract_bootstrap_servers(kafka: &DynamicObject, namespace: &str) -> Result<String> {
    // Try status.listeners first (populated by Strimzi)
    if let Some(status) = kafka.data.get("status") {
        if let Some(listeners) = status.get("listeners") {
            if let Some(listeners_array) = listeners.as_array() {
                // Prefer "plain" or "tls" listener, fall back to first
                for preferred in &["tls", "plain"] {
                    for listener in listeners_array {
                        if let Some(name) = listener.get("name").and_then(|n| n.as_str()) {
                            if name == *preferred {
                                if let Some(bootstrap) =
                                    listener.get("bootstrapServers").and_then(|b| b.as_str())
                                {
                                    return Ok(bootstrap.to_string());
                                }
                            }
                        }
                    }
                }
                // Fall back to first listener with bootstrapServers
                for listener in listeners_array {
                    if let Some(bootstrap) =
                        listener.get("bootstrapServers").and_then(|b| b.as_str())
                    {
                        return Ok(bootstrap.to_string());
                    }
                }
            }
        }
    }

    // Fall back to conventional service name
    let name = kafka.name_any();
    Ok(format!("{name}-kafka-bootstrap.{namespace}.svc:9093"))
}

/// Extract the number of Kafka replicas from the CR spec
fn extract_replicas(kafka: &DynamicObject) -> i32 {
    kafka
        .data
        .get("spec")
        .and_then(|s| s.get("kafka"))
        .and_then(|k| k.get("replicas"))
        .and_then(|r| r.as_i64())
        .unwrap_or(3) as i32
}

/// Extract TLS status and listener name from the Kafka CR
fn extract_listener_info(kafka: &DynamicObject) -> (bool, String) {
    if let Some(spec) = kafka.data.get("spec") {
        if let Some(kafka_spec) = spec.get("kafka") {
            if let Some(listeners) = kafka_spec.get("listeners") {
                if let Some(listeners_array) = listeners.as_array() {
                    // Look for a TLS listener first
                    for listener in listeners_array {
                        if let Some(tls) = listener.get("tls").and_then(|t| t.as_bool()) {
                            if tls {
                                let name = listener
                                    .get("name")
                                    .and_then(|n| n.as_str())
                                    .unwrap_or("tls")
                                    .to_string();
                                return (true, name);
                            }
                        }
                    }
                    // Fall back to first listener
                    if let Some(first) = listeners_array.first() {
                        let name = first
                            .get("name")
                            .and_then(|n| n.as_str())
                            .unwrap_or("plain")
                            .to_string();
                        let tls = first.get("tls").and_then(|t| t.as_bool()).unwrap_or(false);
                        return (tls, name);
                    }
                }
            }
        }
    }
    (true, "tls".to_string())
}
