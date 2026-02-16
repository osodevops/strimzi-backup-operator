use std::collections::BTreeMap;

use k8s_openapi::api::core::v1::{
    ConfigMapVolumeSource, KeyToPath, PodSpec, SecretVolumeSource, Volume, VolumeMount,
};

use crate::crd::common::{PodTemplateSpec as CrdPodTemplate, StorageSpec, StorageType};
use crate::strimzi::kafka_user::ResolvedAuth;
use crate::strimzi::tls;

/// Build standard labels for backup/restore pods
pub fn build_labels(cr_name: &str, cluster_name: &str, job_type: &str) -> BTreeMap<String, String> {
    let mut labels = BTreeMap::new();
    labels.insert(
        "app.kubernetes.io/name".to_string(),
        "kafka-backup-operator".to_string(),
    );
    labels.insert(
        "app.kubernetes.io/instance".to_string(),
        cr_name.to_string(),
    );
    labels.insert(
        "app.kubernetes.io/part-of".to_string(),
        "kafka-backup".to_string(),
    );
    labels.insert(
        "app.kubernetes.io/managed-by".to_string(),
        "kafka-backup-operator".to_string(),
    );
    labels.insert("strimzi.io/cluster".to_string(), cluster_name.to_string());
    labels.insert("kafkabackup.com/type".to_string(), job_type.to_string());
    labels.insert(format!("kafkabackup.com/{job_type}"), cr_name.to_string());
    labels
}

/// Build the volumes and volume mounts for backup/restore containers
pub fn build_volumes_and_mounts(
    config_map_name: &str,
    _config_key: &str,
    cluster_name: &str,
    auth: &ResolvedAuth,
    storage: &StorageSpec,
) -> (Vec<Volume>, Vec<VolumeMount>) {
    let mut volumes = Vec::new();
    let mut mounts = Vec::new();

    // Config volume (from ConfigMap)
    volumes.push(Volume {
        name: "config".to_string(),
        config_map: Some(ConfigMapVolumeSource {
            name: config_map_name.to_string(),
            ..Default::default()
        }),
        ..Default::default()
    });
    mounts.push(VolumeMount {
        name: "config".to_string(),
        mount_path: "/config".to_string(),
        read_only: Some(true),
        ..Default::default()
    });

    // Cluster CA certificate volume
    let ca_secret = tls::cluster_ca_secret_name(cluster_name);
    volumes.push(Volume {
        name: "cluster-ca".to_string(),
        secret: Some(SecretVolumeSource {
            secret_name: Some(ca_secret),
            items: Some(vec![KeyToPath {
                key: "ca.crt".to_string(),
                path: "ca.crt".to_string(),
                ..Default::default()
            }]),
            ..Default::default()
        }),
        ..Default::default()
    });
    mounts.push(VolumeMount {
        name: "cluster-ca".to_string(),
        mount_path: "/certs/cluster-ca".to_string(),
        read_only: Some(true),
        ..Default::default()
    });

    // User credentials volume (TLS or SCRAM)
    match auth {
        ResolvedAuth::Tls { secret_name } => {
            volumes.push(Volume {
                name: "user-certs".to_string(),
                secret: Some(SecretVolumeSource {
                    secret_name: Some(secret_name.clone()),
                    ..Default::default()
                }),
                ..Default::default()
            });
            mounts.push(VolumeMount {
                name: "user-certs".to_string(),
                mount_path: "/certs/user".to_string(),
                read_only: Some(true),
                ..Default::default()
            });
        }
        ResolvedAuth::ScramSha512 {
            secret_name,
            password_key,
            ..
        } => {
            volumes.push(Volume {
                name: "user-certs".to_string(),
                secret: Some(SecretVolumeSource {
                    secret_name: Some(secret_name.clone()),
                    items: Some(vec![KeyToPath {
                        key: password_key.clone(),
                        path: "password".to_string(),
                        ..Default::default()
                    }]),
                    ..Default::default()
                }),
                ..Default::default()
            });
            mounts.push(VolumeMount {
                name: "user-certs".to_string(),
                mount_path: "/certs/user".to_string(),
                read_only: Some(true),
                ..Default::default()
            });
        }
        ResolvedAuth::None => {}
    }

    // Storage credentials volume
    let cred_secret = get_credentials_secret_name(storage);
    if let Some((secret_name, secret_key)) = cred_secret {
        volumes.push(Volume {
            name: "storage-credentials".to_string(),
            secret: Some(SecretVolumeSource {
                secret_name: Some(secret_name),
                items: Some(vec![KeyToPath {
                    key: secret_key,
                    path: "credentials".to_string(),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        });
        mounts.push(VolumeMount {
            name: "storage-credentials".to_string(),
            mount_path: "/credentials".to_string(),
            read_only: Some(true),
            ..Default::default()
        });
    }

    (volumes, mounts)
}

/// Get the storage credentials secret name if configured
fn get_credentials_secret_name(storage: &StorageSpec) -> Option<(String, String)> {
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

/// Apply pod template overrides from the CRD to the pod spec and its first container.
/// Uses serde_json conversion for pass-through k8s types.
pub fn apply_pod_template(pod_spec: &mut PodSpec, template: Option<&CrdPodTemplate>) {
    let Some(tmpl) = template else { return };

    if let Some(pod_overrides) = &tmpl.pod {
        if let Some(affinity) = &pod_overrides.affinity {
            if let Ok(a) = serde_json::from_value(affinity.clone()) {
                pod_spec.affinity = Some(a);
            }
        }

        if !pod_overrides.tolerations.is_empty() {
            let tolerations: Vec<k8s_openapi::api::core::v1::Toleration> = pod_overrides
                .tolerations
                .iter()
                .filter_map(|t| serde_json::from_value(t.clone()).ok())
                .collect();
            if !tolerations.is_empty() {
                pod_spec.tolerations = Some(tolerations);
            }
        }

        if let Some(sc) = &pod_overrides.security_context {
            if let Ok(s) = serde_json::from_value(sc.clone()) {
                pod_spec.security_context = Some(s);
            }
        }

        if !pod_overrides.image_pull_secrets.is_empty() {
            let secrets: Vec<k8s_openapi::api::core::v1::LocalObjectReference> = pod_overrides
                .image_pull_secrets
                .iter()
                .filter_map(|s| serde_json::from_value(s.clone()).ok())
                .collect();
            if !secrets.is_empty() {
                pod_spec.image_pull_secrets = Some(secrets);
            }
        }
    }

    if let Some(container_overrides) = &tmpl.container {
        if let Some(container) = pod_spec.containers.first_mut() {
            if !container_overrides.env.is_empty() {
                let env_vars: Vec<k8s_openapi::api::core::v1::EnvVar> = container_overrides
                    .env
                    .iter()
                    .filter_map(|e| serde_json::from_value(e.clone()).ok())
                    .collect();
                let existing_env = container.env.get_or_insert_with(Vec::new);
                existing_env.extend(env_vars);
            }

            if let Some(sc) = &container_overrides.security_context {
                if let Ok(s) = serde_json::from_value(sc.clone()) {
                    container.security_context = Some(s);
                }
            }
        }
    }
}

/// Build annotations map combining standard and template annotations
pub fn build_annotations(template: Option<&CrdPodTemplate>) -> BTreeMap<String, String> {
    let mut annotations = BTreeMap::new();

    if let Some(tmpl) = template {
        if let Some(pod) = &tmpl.pod {
            if let Some(meta) = &pod.metadata {
                annotations.extend(meta.annotations.clone());
            }
        }
    }

    annotations
}

/// Merge template labels with standard labels
pub fn merge_template_labels(
    labels: &mut BTreeMap<String, String>,
    template: Option<&CrdPodTemplate>,
) {
    if let Some(tmpl) = template {
        if let Some(pod) = &tmpl.pod {
            if let Some(meta) = &pod.metadata {
                labels.extend(meta.labels.clone());
            }
        }
    }
}
