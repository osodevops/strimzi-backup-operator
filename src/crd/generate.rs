//! CRD generation for `crdgen` and the tests that pin the served versions.
//!
//! Each kind is served under two API versions with an identical schema:
//! [`STORAGE_VERSION`] (`v1`, stable) and [`DEPRECATED_VERSION`] (`v1alpha1`,
//! deprecated with a `deprecationWarning`). No conversion webhook is needed
//! while the schemas are identical (`conversion.strategy: None`).

use k8s_openapi::apiextensions_apiserver::pkg::apis::apiextensions::v1::CustomResourceDefinition;
use kube::core::crd::merge_crds;
use kube::CustomResourceExt;

use super::{kafka_backup, kafka_restore};

/// The version objects are stored as and that the operator watches.
pub const STORAGE_VERSION: &str = "v1";
/// Still served, but every request receives a deprecation warning.
pub const DEPRECATED_VERSION: &str = "v1alpha1";

fn deprecate(crd: &mut CustomResourceDefinition, kind: &str) {
    for version in crd.spec.versions.iter_mut() {
        if version.name == DEPRECATED_VERSION {
            version.deprecated = Some(true);
            version.deprecation_warning = Some(format!(
                "kafkabackup.com/{DEPRECATED_VERSION} {kind} is deprecated; use kafkabackup.com/{STORAGE_VERSION} (identical schema — only apiVersion changes)"
            ));
        }
    }
}

/// The merged `KafkaBackup` CRD (`v1` storage, `v1alpha1` deprecated).
pub fn kafka_backup_crd() -> CustomResourceDefinition {
    let mut crd = merge_crds(
        vec![
            kafka_backup::v1::KafkaBackup::crd(),
            kafka_backup::v1alpha1::KafkaBackup::crd(),
        ],
        STORAGE_VERSION,
    )
    .expect("KafkaBackup CRD versions merge");
    deprecate(&mut crd, "KafkaBackup");
    crd
}

/// The merged `KafkaRestore` CRD (`v1` storage, `v1alpha1` deprecated).
pub fn kafka_restore_crd() -> CustomResourceDefinition {
    let mut crd = merge_crds(
        vec![
            kafka_restore::v1::KafkaRestore::crd(),
            kafka_restore::v1alpha1::KafkaRestore::crd(),
        ],
        STORAGE_VERSION,
    )
    .expect("KafkaRestore CRD versions merge");
    deprecate(&mut crd, "KafkaRestore");
    crd
}

/// Plain YAML, as installed with `kubectl apply -f deploy/crds/`.
pub fn to_yaml(crd: &CustomResourceDefinition) -> String {
    serde_yaml::to_string(crd).expect("CRD serializes to YAML")
}

/// The Helm-templated variant written to `templates/crds/`: gated by
/// `crds.install`, and annotated `helm.sh/resource-policy: keep` when
/// `crds.keep` is set so `helm uninstall` leaves the CRDs (and every
/// KafkaBackup/KafkaRestore) in place. Rendering the CRDs as templates —
/// instead of the chart's static `crds/` directory, which Helm only applies
/// on install — is what lets `helm upgrade` add the `v1` version.
pub fn to_helm_template(crd: &CustomResourceDefinition) -> String {
    let yaml = to_yaml(crd);
    let name = crd.metadata.name.as_deref().expect("CRD has a name");
    let name_block = format!("metadata:\n  name: {name}\n");
    assert!(
        yaml.contains(&name_block),
        "unexpected CRD metadata layout: {yaml}"
    );
    let annotated = yaml.replacen(
        &name_block,
        &format!(
            "{name_block}  {{{{- if .Values.crds.keep }}}}\n  annotations:\n    helm.sh/resource-policy: keep\n  {{{{- end }}}}\n"
        ),
        1,
    );
    format!("{{{{- if .Values.crds.install }}}}\n{annotated}{{{{- end }}}}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_versions(crd: &CustomResourceDefinition, kind: &str) {
        let names: Vec<&str> = crd.spec.versions.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names.len(), 2, "{kind}: {names:?}");
        assert!(names.contains(&STORAGE_VERSION) && names.contains(&DEPRECATED_VERSION));
        for version in &crd.spec.versions {
            assert!(version.served, "{kind} {}: must be served", version.name);
            let is_storage = version.name == STORAGE_VERSION;
            assert_eq!(
                version.storage, is_storage,
                "{kind} {}: storage flag",
                version.name
            );
            assert_eq!(
                version.deprecated.unwrap_or(false),
                !is_storage,
                "{kind} {}: deprecated flag",
                version.name
            );
            if !is_storage {
                let warning = version.deprecation_warning.as_deref().unwrap_or("");
                assert!(warning.contains("use kafkabackup.com/v1"), "{warning}");
            }
            assert!(
                version
                    .subresources
                    .as_ref()
                    .is_some_and(|s| s.status.is_some()),
                "{kind} {}: status subresource",
                version.name
            );
        }
        let schemas: Vec<serde_json::Value> = crd
            .spec
            .versions
            .iter()
            .map(|v| serde_json::to_value(&v.schema).unwrap())
            .collect();
        assert_eq!(schemas[0], schemas[1], "{kind}: schemas must be identical");
        assert!(
            crd.spec.conversion.is_none(),
            "{kind}: identical schemas need no conversion webhook"
        );
    }

    #[test]
    fn kafka_backup_serves_v1_storage_and_deprecated_v1alpha1() {
        let crd = kafka_backup_crd();
        assert_eq!(
            crd.metadata.name.as_deref(),
            Some("kafkabackups.kafkabackup.com")
        );
        check_versions(&crd, "KafkaBackup");
    }

    #[test]
    fn kafka_restore_serves_v1_storage_and_deprecated_v1alpha1() {
        let crd = kafka_restore_crd();
        assert_eq!(
            crd.metadata.name.as_deref(),
            Some("kafkarestores.kafkabackup.com")
        );
        check_versions(&crd, "KafkaRestore");
    }

    #[test]
    fn helm_template_is_gated_and_keeps_crds() {
        let rendered = to_helm_template(&kafka_backup_crd());
        assert!(rendered.starts_with("{{- if .Values.crds.install }}\n"));
        assert!(rendered.trim_end().ends_with("{{- end }}"));
        assert!(rendered.contains("  {{- if .Values.crds.keep }}\n  annotations:\n    helm.sh/resource-policy: keep\n  {{- end }}\n"));
        assert!(rendered.contains("name: kafkabackups.kafkabackup.com"));
        // The plain YAML must round-trip untouched inside the template.
        let plain = to_yaml(&kafka_backup_crd());
        for line in plain.lines().filter(|l| l.contains("name: v1")) {
            assert!(rendered.contains(line));
        }
    }
}
