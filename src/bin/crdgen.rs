//! Writes the CRDs twice: plain YAML under `deploy/crds/` (for `kubectl
//! apply` and the `crds.yaml` release asset) and the Helm-templated variant
//! under `deploy/helm/strimzi-backup-operator/templates/crds/`. CI fails if
//! either is out of date.

use std::fs;
use std::path::Path;

use kafka_backup_operator::crd::generate::{
    kafka_backup_crd, kafka_restore_crd, to_helm_template, to_yaml,
};

fn main() {
    let crds_dir = Path::new("deploy/crds");
    let helm_dir = Path::new("deploy/helm/strimzi-backup-operator/templates/crds");
    fs::create_dir_all(crds_dir).expect("Failed to create deploy/crds directory");
    fs::create_dir_all(helm_dir).expect("Failed to create the Helm templates/crds directory");

    for (file, crd) in [
        ("kafkabackups.yaml", kafka_backup_crd()),
        ("kafkarestores.yaml", kafka_restore_crd()),
    ] {
        fs::write(crds_dir.join(file), to_yaml(&crd))
            .unwrap_or_else(|e| panic!("Failed to write {}: {e}", crds_dir.join(file).display()));
        println!("Generated {}", crds_dir.join(file).display());

        fs::write(helm_dir.join(file), to_helm_template(&crd))
            .unwrap_or_else(|e| panic!("Failed to write {}: {e}", helm_dir.join(file).display()));
        println!("Generated {}", helm_dir.join(file).display());
    }
}
