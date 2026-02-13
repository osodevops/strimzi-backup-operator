use kube::CustomResourceExt;
use std::fs;
use std::path::Path;

use strimzi_backup_operator::crd::{KafkaBackup, KafkaRestore};

fn main() {
    let crds_dir = Path::new("deploy/crds");
    fs::create_dir_all(crds_dir).expect("Failed to create deploy/crds directory");

    let backup_crd =
        serde_yaml::to_string(&KafkaBackup::crd()).expect("Failed to serialize KafkaBackup CRD");
    fs::write(crds_dir.join("kafkabackups.yaml"), backup_crd)
        .expect("Failed to write KafkaBackup CRD");
    println!("Generated deploy/crds/kafkabackups.yaml");

    let restore_crd =
        serde_yaml::to_string(&KafkaRestore::crd()).expect("Failed to serialize KafkaRestore CRD");
    fs::write(crds_dir.join("kafkarestores.yaml"), restore_crd)
        .expect("Failed to write KafkaRestore CRD");
    println!("Generated deploy/crds/kafkarestores.yaml");
}
