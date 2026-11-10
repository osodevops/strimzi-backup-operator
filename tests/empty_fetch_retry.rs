//! Core #256 must be fixed in the CLI image and in generated Job configs.
use kafka_backup_operator::adapters::backup_config::build_backup_config_yaml;
use kafka_backup_operator::adapters::restore_config::build_restore_config_yaml;
use kafka_backup_operator::crd::{KafkaBackup, KafkaRestore};
use kafka_backup_operator::engine::{EngineVersion, DEFAULT_BACKUP_IMAGE};
use kafka_backup_operator::strimzi::kafka_cr::ResolvedKafkaCluster;
use kafka_backup_operator::strimzi::kafka_user::ResolvedAuth;
use serde_json::json;

fn cluster() -> ResolvedKafkaCluster {
    ResolvedKafkaCluster {
        name: "kafka".into(),
        namespace: "default".into(),
        bootstrap_servers: "kafka:9092".into(),
        replicas: 1,
        tls_enabled: false,
        listener_name: "plain".into(),
    }
}
fn backup(retry: u64, extra: serde_json::Value) -> KafkaBackup {
    let spec = json!({"strimziClusterRef":{"name":"kafka"}, "storage":{"type":"filesystem","filesystem":{"path":"/backups"}}, "connection":{"fetchMaxWaitMs":5000}, "backup":{"emptyFetchRetrySecs":retry,"config":extra}});
    KafkaBackup::new("cold", serde_json::from_value(spec).unwrap())
}
fn yaml(backup: &KafkaBackup) -> serde_yaml::Value {
    serde_yaml::from_str(
        &build_backup_config_yaml(backup, &cluster(), &None, &ResolvedAuth::None).unwrap(),
    )
    .unwrap()
}
#[test]
fn default_engine_contains_the_empty_fetch_fix() {
    let version = EngineVersion::parse(DEFAULT_BACKUP_IMAGE.rsplit(':').next().unwrap()).unwrap();
    assert!(
        version
            >= EngineVersion {
                major: 0,
                minor: 28,
                patch: 0
            }
    );
}
#[test]
fn typed_budget_and_fetch_wait_reach_backup_jobs_including_zero() {
    for retry in [0, 30, 3600] {
        let c = yaml(&backup(retry, json!({})));
        assert_eq!(
            c["backup"]["empty_fetch_retry_secs"],
            serde_yaml::Value::from(retry)
        );
        assert_eq!(
            c["source"]["connection"]["fetch_max_wait_ms"],
            serde_yaml::Value::from(5000)
        );
    }
}
#[test]
fn native_option_override_retains_precedence() {
    let c = yaml(&backup(30, json!({"empty_fetch_retry_secs":60})));
    assert_eq!(
        c["backup"]["empty_fetch_retry_secs"],
        serde_yaml::Value::from(60)
    );
}
#[test]
fn fetch_wait_reaches_restore_jobs() {
    let restore = KafkaRestore::new("restore", serde_json::from_value(json!({"strimziClusterRef":{"name":"kafka"},"backupRef":{"name":"cold","backupId":"cold"},"connection":{"fetchMaxWaitMs":5000}})).unwrap());
    let c: serde_yaml::Value = serde_yaml::from_str(
        &build_restore_config_yaml(
            &restore,
            &backup(30, json!({})),
            &cluster(),
            &None,
            &ResolvedAuth::None,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        c["target"]["connection"]["fetch_max_wait_ms"],
        serde_yaml::Value::from(5000)
    );
}
#[test]
fn both_served_apis_validate_the_new_bounds() {
    use kafka_backup_operator::crd::generate::{kafka_backup_crd, kafka_restore_crd};
    for (crd, is_backup) in [(kafka_backup_crd(), true), (kafka_restore_crd(), false)] {
        let crd = serde_json::to_value(crd).unwrap();
        for version in crd["spec"]["versions"].as_array().unwrap() {
            let props = &version["schema"]["openAPIV3Schema"]["properties"]["spec"]["properties"];
            let wait = &props["connection"]["properties"]["fetchMaxWaitMs"];
            assert_eq!(wait["minimum"], json!(0.0));
            assert_eq!(wait["maximum"], json!(30000.0));
            if is_backup {
                let retry = &props["backup"]["properties"]["emptyFetchRetrySecs"];
                assert_eq!(retry["minimum"], json!(0.0));
                assert_eq!(retry["maximum"], json!(3600.0));
            }
        }
    }
}
