use kube::{
    api::{Api, DynamicObject, GroupVersionKind},
    Client, ResourceExt,
};
use tracing::{debug, info};

use crate::crd::common::{AuthenticationType, StrimziClusterRef};
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

/// Resolve a Strimzi Kafka CR reference to get cluster connection details.
///
/// `desired_auth` is the authentication type declared on the KafkaBackup or
/// KafkaRestore: the listener is chosen to match it, so SCRAM credentials are
/// never pointed at an mTLS listener and vice versa (issue #37).
pub async fn resolve_kafka_cluster(
    client: &Client,
    cluster_ref: &StrimziClusterRef,
    default_namespace: &str,
    desired_auth: Option<&AuthenticationType>,
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

    let connection = resolve_connection(
        &kafka,
        namespace,
        desired_auth,
        cluster_ref.listener.as_deref(),
    )?;
    let replicas = extract_replicas(&kafka);

    let resolved = ResolvedKafkaCluster {
        name: name.clone(),
        namespace: namespace.to_string(),
        bootstrap_servers: connection.bootstrap_servers,
        replicas,
        tls_enabled: connection.tls_enabled,
        listener_name: connection.listener_name,
    };

    debug!(?resolved, "Resolved Kafka cluster");
    Ok(resolved)
}

/// Connection details derived from one listener of the Kafka CR
#[derive(Debug, PartialEq)]
struct ResolvedConnection {
    bootstrap_servers: String,
    tls_enabled: bool,
    listener_name: String,
}

/// A listener chosen from `spec.kafka.listeners`
struct SelectedListener {
    name: String,
    port: i64,
    tls: bool,
}

/// Pick a listener compatible with the desired authentication and derive the
/// bootstrap address for it from the Kafka CR status.
fn resolve_connection(
    kafka: &DynamicObject,
    namespace: &str,
    desired_auth: Option<&AuthenticationType>,
    listener_override: Option<&str>,
) -> Result<ResolvedConnection> {
    let cluster_name = kafka.name_any();
    let listeners = kafka
        .data
        .get("spec")
        .and_then(|s| s.get("kafka"))
        .and_then(|k| k.get("listeners"))
        .and_then(|l| l.as_array())
        .filter(|l| !l.is_empty())
        .ok_or_else(|| {
            Error::InvalidConfig(format!(
                "Kafka cluster '{cluster_name}' has no spec.kafka.listeners"
            ))
        })?;

    let selected = select_listener(listeners, desired_auth, listener_override, &cluster_name)?;
    let bootstrap_servers = bootstrap_for_listener(kafka, namespace, &cluster_name, &selected);

    Ok(ResolvedConnection {
        bootstrap_servers,
        tls_enabled: selected.tls,
        listener_name: selected.name,
    })
}

/// Select the listener to connect through.
///
/// With an explicit override the named listener is used as-is. Otherwise only
/// listeners whose `authentication.type` matches the desired authentication
/// are considered — connecting with mismatched credentials can never succeed
/// (an mTLS listener rejects the TLS handshake of a SCRAM client outright,
/// issue #37). Among compatible listeners, in-cluster reachable types
/// (`internal`, `cluster-ip`) win over external ones since job pods run inside
/// the cluster, then TLS-encrypted listeners win over plaintext, then spec
/// order breaks ties.
fn select_listener(
    listeners: &[serde_json::Value],
    desired_auth: Option<&AuthenticationType>,
    listener_override: Option<&str>,
    cluster_name: &str,
) -> Result<SelectedListener> {
    if let Some(override_name) = listener_override {
        let listener = listeners
            .iter()
            .find(|l| listener_name(l) == Some(override_name))
            .ok_or_else(|| Error::InvalidConfig(format!(
                "listener '{override_name}' from spec.strimziClusterRef.listener not found on Kafka cluster '{cluster_name}'; available listeners: {}",
                summarize_listeners(listeners)
            )))?;
        return Ok(to_selected(listener));
    }

    let best = listeners
        .iter()
        .enumerate()
        .filter(|(_, l)| auth_matches(l, desired_auth))
        .min_by_key(|(idx, l)| {
            let external = !is_in_cluster(l);
            let plaintext = !listener_tls(l);
            (external, plaintext, *idx)
        });

    match best {
        Some((_, listener)) => Ok(to_selected(listener)),
        None => Err(Error::NoCompatibleListener {
            cluster: cluster_name.to_string(),
            desired: desired_auth_label(desired_auth).to_string(),
            available: summarize_listeners(listeners),
        }),
    }
}

/// Whether a listener's authentication matches the CR's authentication type
fn auth_matches(listener: &serde_json::Value, desired: Option<&AuthenticationType>) -> bool {
    let listener_auth = listener
        .get("authentication")
        .and_then(|a| a.get("type"))
        .and_then(|t| t.as_str());
    match desired {
        None => listener_auth.is_none(),
        Some(AuthenticationType::Tls) => listener_auth == Some("tls"),
        Some(AuthenticationType::ScramSha512) => listener_auth == Some("scram-sha-512"),
    }
}

/// Whether the listener's bootstrap address is reachable from pods in the
/// cluster. A missing `type` is treated as internal to stay permissive.
fn is_in_cluster(listener: &serde_json::Value) -> bool {
    match listener.get("type").and_then(|t| t.as_str()) {
        Some("internal") | Some("cluster-ip") | None => true,
        Some(_) => false,
    }
}

fn listener_name(listener: &serde_json::Value) -> Option<&str> {
    listener.get("name").and_then(|n| n.as_str())
}

fn listener_tls(listener: &serde_json::Value) -> bool {
    listener
        .get("tls")
        .and_then(|t| t.as_bool())
        .unwrap_or(false)
}

fn to_selected(listener: &serde_json::Value) -> SelectedListener {
    SelectedListener {
        name: listener_name(listener).unwrap_or("unnamed").to_string(),
        port: listener
            .get("port")
            .and_then(|p| p.as_i64())
            .unwrap_or(9092),
        tls: listener_tls(listener),
    }
}

fn desired_auth_label(desired: Option<&AuthenticationType>) -> &'static str {
    match desired {
        None => "none",
        Some(AuthenticationType::Tls) => "tls",
        Some(AuthenticationType::ScramSha512) => "scram-sha-512",
    }
}

/// Human-readable listener list for error messages, e.g.
/// `plain (port 9092, tls: false, auth: scram-sha-512), tls (port 9093, tls: true, auth: tls)`
fn summarize_listeners(listeners: &[serde_json::Value]) -> String {
    listeners
        .iter()
        .map(|l| {
            let auth = l
                .get("authentication")
                .and_then(|a| a.get("type"))
                .and_then(|t| t.as_str())
                .unwrap_or("none");
            format!(
                "{} (port {}, tls: {}, auth: {})",
                listener_name(l).unwrap_or("unnamed"),
                l.get("port").and_then(|p| p.as_i64()).unwrap_or(0),
                listener_tls(l),
                auth
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Bootstrap address for the selected listener: the matching entry in
/// `status.listeners` (correlated by name), falling back to the conventional
/// bootstrap service with the listener's declared port when the status is not
/// populated yet.
fn bootstrap_for_listener(
    kafka: &DynamicObject,
    namespace: &str,
    cluster_name: &str,
    selected: &SelectedListener,
) -> String {
    let from_status = kafka
        .data
        .get("status")
        .and_then(|s| s.get("listeners"))
        .and_then(|l| l.as_array())
        .and_then(|listeners| {
            listeners
                .iter()
                .find(|l| listener_name(l) == Some(selected.name.as_str()))
                .and_then(|l| l.get("bootstrapServers"))
                .and_then(|b| b.as_str())
                .map(str::to_string)
        });

    from_status.unwrap_or_else(|| {
        format!(
            "{cluster_name}-kafka-bootstrap.{namespace}.svc:{}",
            selected.port
        )
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use kube::api::ApiResource;
    use serde_json::json;

    fn kafka_fixture(
        spec_listeners: serde_json::Value,
        status_listeners: Option<serde_json::Value>,
    ) -> DynamicObject {
        let ar = ApiResource::from_gvk(&GroupVersionKind::gvk(
            "kafka.strimzi.io",
            "v1beta2",
            "Kafka",
        ));
        let mut obj = DynamicObject::new("my-cluster", &ar);
        obj.data = json!({"spec": {"kafka": {"listeners": spec_listeners}}});
        if let Some(status) = status_listeners {
            obj.data["status"] = json!({ "listeners": status });
        }
        obj
    }

    /// The reporter's cluster in issue #37: a SCRAM listener next to an mTLS
    /// listener. SCRAM credentials must go to the SCRAM listener.
    fn dual_listener_spec() -> serde_json::Value {
        json!([
            {"name": "plain", "port": 9092, "type": "internal", "tls": false,
             "authentication": {"type": "scram-sha-512"}},
            {"name": "tls", "port": 9093, "type": "internal", "tls": true,
             "authentication": {"type": "tls"}},
        ])
    }

    fn dual_listener_status() -> serde_json::Value {
        json!([
            {"name": "plain", "bootstrapServers": "my-cluster-kafka-bootstrap.kafka.svc:9092"},
            {"name": "tls", "bootstrapServers": "my-cluster-kafka-bootstrap.kafka.svc:9093"},
        ])
    }

    #[test]
    fn scram_auth_selects_scram_listener_not_mtls() {
        let kafka = kafka_fixture(dual_listener_spec(), Some(dual_listener_status()));
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap();
        assert_eq!(conn.listener_name, "plain");
        assert!(!conn.tls_enabled);
        assert_eq!(
            conn.bootstrap_servers,
            "my-cluster-kafka-bootstrap.kafka.svc:9092"
        );
    }

    #[test]
    fn tls_auth_selects_mtls_listener() {
        let kafka = kafka_fixture(dual_listener_spec(), Some(dual_listener_status()));
        let conn =
            resolve_connection(&kafka, "kafka", Some(&AuthenticationType::Tls), None).unwrap();
        assert_eq!(conn.listener_name, "tls");
        assert!(conn.tls_enabled);
        assert_eq!(
            conn.bootstrap_servers,
            "my-cluster-kafka-bootstrap.kafka.svc:9093"
        );
    }

    #[test]
    fn scram_prefers_tls_encrypted_scram_listener_over_plaintext() {
        let kafka = kafka_fixture(
            json!([
                {"name": "plain", "port": 9092, "type": "internal", "tls": false,
                 "authentication": {"type": "scram-sha-512"}},
                {"name": "scramtls", "port": 9094, "type": "internal", "tls": true,
                 "authentication": {"type": "scram-sha-512"}},
            ]),
            Some(json!([
                {"name": "plain", "bootstrapServers": "my-cluster-kafka-bootstrap.kafka.svc:9092"},
                {"name": "scramtls", "bootstrapServers": "my-cluster-kafka-bootstrap.kafka.svc:9094"},
            ])),
        );
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap();
        assert_eq!(conn.listener_name, "scramtls");
        assert!(conn.tls_enabled);
        assert_eq!(
            conn.bootstrap_servers,
            "my-cluster-kafka-bootstrap.kafka.svc:9094"
        );
    }

    #[test]
    fn scram_prefers_in_cluster_listener_over_external() {
        let kafka = kafka_fixture(
            json!([
                {"name": "external", "port": 9095, "type": "nodeport", "tls": true,
                 "authentication": {"type": "scram-sha-512"}},
                {"name": "plain", "port": 9092, "type": "internal", "tls": false,
                 "authentication": {"type": "scram-sha-512"}},
            ]),
            None,
        );
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap();
        assert_eq!(conn.listener_name, "plain");
    }

    #[test]
    fn external_listener_used_when_only_compatible_option() {
        let kafka = kafka_fixture(
            json!([
                {"name": "external", "port": 9095, "type": "loadbalancer", "tls": true,
                 "authentication": {"type": "scram-sha-512"}},
                {"name": "tls", "port": 9093, "type": "internal", "tls": true,
                 "authentication": {"type": "tls"}},
            ]),
            None,
        );
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap();
        assert_eq!(conn.listener_name, "external");
    }

    #[test]
    fn no_auth_selects_unauthenticated_listener() {
        let kafka = kafka_fixture(
            json!([
                {"name": "plain", "port": 9092, "type": "internal", "tls": false,
                 "authentication": {"type": "scram-sha-512"}},
                {"name": "open", "port": 9096, "type": "internal", "tls": false},
            ]),
            None,
        );
        let conn = resolve_connection(&kafka, "kafka", None, None).unwrap();
        assert_eq!(conn.listener_name, "open");
        assert!(!conn.tls_enabled);
    }

    #[test]
    fn no_auth_prefers_tls_encrypted_unauthenticated_listener() {
        let kafka = kafka_fixture(
            json!([
                {"name": "open", "port": 9096, "type": "internal", "tls": false},
                {"name": "encrypted", "port": 9097, "type": "internal", "tls": true},
            ]),
            None,
        );
        let conn = resolve_connection(&kafka, "kafka", None, None).unwrap();
        assert_eq!(conn.listener_name, "encrypted");
        assert!(conn.tls_enabled);
    }

    #[test]
    fn no_compatible_listener_is_actionable_error() {
        let kafka = kafka_fixture(
            json!([
                {"name": "tls", "port": 9093, "type": "internal", "tls": true,
                 "authentication": {"type": "tls"}},
            ]),
            None,
        );
        let err = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("scram-sha-512"),
            "mentions desired auth: {msg}"
        );
        assert!(
            msg.contains("tls (port 9093, tls: true, auth: tls)"),
            "lists available listeners: {msg}"
        );
        assert!(
            msg.contains("strimziClusterRef.listener"),
            "points at the override: {msg}"
        );
        assert_eq!(err.reason(), "NoCompatibleListener");
    }

    #[test]
    fn oauth_and_custom_listeners_are_never_auto_selected() {
        let kafka = kafka_fixture(
            json!([
                {"name": "oauth", "port": 9098, "type": "internal", "tls": true,
                 "authentication": {"type": "oauth"}},
                {"name": "custom", "port": 9099, "type": "internal", "tls": true,
                 "authentication": {"type": "custom"}},
            ]),
            None,
        );
        for desired in [
            None,
            Some(&AuthenticationType::Tls),
            Some(&AuthenticationType::ScramSha512),
        ] {
            let result = resolve_connection(&kafka, "kafka", desired, None);
            assert!(
                result.is_err(),
                "desired {desired:?} must not match oauth/custom"
            );
        }
    }

    #[test]
    fn listener_override_wins_over_auto_selection() {
        let kafka = kafka_fixture(dual_listener_spec(), Some(dual_listener_status()));
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            Some("tls"),
        )
        .unwrap();
        assert_eq!(conn.listener_name, "tls");
        assert!(conn.tls_enabled);
        assert_eq!(
            conn.bootstrap_servers,
            "my-cluster-kafka-bootstrap.kafka.svc:9093"
        );
    }

    #[test]
    fn unknown_listener_override_is_actionable_error() {
        let kafka = kafka_fixture(dual_listener_spec(), None);
        let err = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            Some("nope"),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("'nope'"), "names the bad override: {msg}");
        assert!(msg.contains("plain"), "lists available listeners: {msg}");
    }

    #[test]
    fn bootstrap_correlates_status_by_name_not_position() {
        // status order deliberately reversed relative to spec
        let kafka = kafka_fixture(
            dual_listener_spec(),
            Some(json!([
                {"name": "tls", "bootstrapServers": "my-cluster-kafka-bootstrap.kafka.svc:9093"},
                {"name": "plain", "bootstrapServers": "my-cluster-kafka-bootstrap.kafka.svc:9092"},
            ])),
        );
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap();
        assert_eq!(
            conn.bootstrap_servers,
            "my-cluster-kafka-bootstrap.kafka.svc:9092"
        );
    }

    #[test]
    fn missing_status_falls_back_to_convention_with_listener_port() {
        let kafka = kafka_fixture(dual_listener_spec(), None);
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap();
        // port 9092 from the selected plain listener, not the old hardcoded 9093
        assert_eq!(
            conn.bootstrap_servers,
            "my-cluster-kafka-bootstrap.kafka.svc:9092"
        );
    }

    #[test]
    fn status_entry_for_other_listener_only_falls_back_to_convention() {
        // status has an entry, but not for the selected listener
        let kafka = kafka_fixture(
            dual_listener_spec(),
            Some(json!([
                {"name": "tls", "bootstrapServers": "my-cluster-kafka-bootstrap.kafka.svc:9093"},
            ])),
        );
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap();
        assert_eq!(
            conn.bootstrap_servers,
            "my-cluster-kafka-bootstrap.kafka.svc:9092"
        );
    }

    #[test]
    fn no_listeners_is_error() {
        let kafka = kafka_fixture(json!([]), None);
        let err = resolve_connection(&kafka, "kafka", None, None).unwrap_err();
        assert!(err.to_string().contains("no spec.kafka.listeners"));
    }

    #[test]
    fn spec_order_breaks_ties_deterministically() {
        let kafka = kafka_fixture(
            json!([
                {"name": "scram-a", "port": 9092, "type": "internal", "tls": true,
                 "authentication": {"type": "scram-sha-512"}},
                {"name": "scram-b", "port": 9094, "type": "internal", "tls": true,
                 "authentication": {"type": "scram-sha-512"}},
            ]),
            None,
        );
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap();
        assert_eq!(conn.listener_name, "scram-a");
    }

    #[test]
    fn missing_listener_type_treated_as_internal() {
        let kafka = kafka_fixture(
            json!([
                {"name": "external", "port": 9095, "type": "nodeport", "tls": true,
                 "authentication": {"type": "scram-sha-512"}},
                {"name": "untyped", "port": 9092, "tls": false,
                 "authentication": {"type": "scram-sha-512"}},
            ]),
            None,
        );
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap();
        assert_eq!(conn.listener_name, "untyped");
    }

    /// The single-listener SCRAM setup from issue #35's e2e must keep working
    /// exactly as before the fix.
    #[test]
    fn single_scram_listener_still_selected() {
        let kafka = kafka_fixture(
            json!([
                {"name": "plain", "port": 9092, "type": "internal", "tls": false,
                 "authentication": {"type": "scram-sha-512"}},
            ]),
            Some(json!([
                {"name": "plain", "bootstrapServers": "my-cluster-kafka-bootstrap.kafka.svc:9092"},
            ])),
        );
        let conn = resolve_connection(
            &kafka,
            "kafka",
            Some(&AuthenticationType::ScramSha512),
            None,
        )
        .unwrap();
        assert_eq!(conn.listener_name, "plain");
        assert!(!conn.tls_enabled);
    }

    /// Encryption-only TLS listener (no authentication block) matches a CR
    /// without spec.authentication — the pre-fix behavior for anonymous SSL.
    #[test]
    fn no_auth_matches_encryption_only_tls_listener() {
        let kafka = kafka_fixture(
            json!([
                {"name": "tls", "port": 9093, "type": "internal", "tls": true},
            ]),
            Some(json!([
                {"name": "tls", "bootstrapServers": "my-cluster-kafka-bootstrap.kafka.svc:9093"},
            ])),
        );
        let conn = resolve_connection(&kafka, "kafka", None, None).unwrap();
        assert_eq!(conn.listener_name, "tls");
        assert!(conn.tls_enabled);
    }
}
