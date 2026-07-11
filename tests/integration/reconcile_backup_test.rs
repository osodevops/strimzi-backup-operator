use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use http::{Request, Response};
use http_body_util::BodyExt;
use kafka_backup_operator::crd::common::*;
use kafka_backup_operator::crd::kafka_backup::*;
use kafka_backup_operator::crd::KafkaBackup;
use kafka_backup_operator::metrics::prometheus::MetricsState;
use kafka_backup_operator::reconcilers::backup::reconcile_backup;
use kube::client::Body;
use kube::Client;
use serde_json::json;
use tower_test::mock;

#[derive(Debug)]
struct RecordedRequest {
    method: String,
    path: String,
    query: Option<String>,
    body: serde_json::Value,
}

fn scheduled_backup(suspend: bool) -> KafkaBackup {
    let spec = KafkaBackupSpec {
        strimzi_cluster_ref: StrimziClusterRef {
            name: "production-cluster".to_string(),
            namespace: None,
            ca_secret: None,
            listener: None,
        },
        authentication: None,
        topics: None,
        connection: None,
        consumer_groups: None,
        logging: None,
        env: Vec::new(),
        storage: StorageSpec {
            storage_type: StorageType::Filesystem,
            s3: None,
            azure: None,
            gcs: None,
            filesystem: Some(FilesystemStorageSpec {
                path: "/backups".to_string(),
            }),
        },
        backup: None,
        metrics: None,
        offset_storage: None,
        schedule: Some(ScheduleSpec {
            cron: "0 2 * * *".to_string(),
            timezone: None,
            suspend,
        }),
        retention: None,
        resources: None,
        template: None,
        image: None,
        backoff_limit: None,
    };
    let mut backup = KafkaBackup::new("daily-backup", spec);
    backup.metadata.namespace = Some("kafka".to_string());
    backup.metadata.uid = Some("test-uid-12345".to_string());
    backup.metadata.generation = Some(2);
    backup.metadata.finalizers = Some(vec!["kafkabackup.com/cleanup".to_string()]);
    backup
}

/// Run `reconcile_backup` against a mock API server and record every request
/// the operator makes.
async fn reconcile_with_mock_api(backup: KafkaBackup) -> Vec<RecordedRequest> {
    let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
    let recorded = Arc::new(Mutex::new(Vec::new()));

    let backup_json = serde_json::to_value(&backup).unwrap();
    let dispatcher = {
        let recorded = Arc::clone(&recorded);
        tokio::spawn(async move {
            while let Some((request, send)) = handle.next_request().await {
                let method = request.method().to_string();
                let path = request.uri().path().to_string();
                let query = request.uri().query().map(str::to_string);
                let bytes = request.into_body().collect().await.unwrap().to_bytes();
                let body: serde_json::Value = if bytes.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_slice(&bytes).unwrap()
                };

                let (status, response_body) = if path.ends_with("/kafkas/production-cluster") {
                    (
                        200,
                        json!({
                            "apiVersion": "kafka.strimzi.io/v1beta2",
                            "kind": "Kafka",
                            "metadata": {"name": "production-cluster", "namespace": "kafka"},
                            "spec": {"kafka": {"replicas": 3, "listeners": [
                                {"name": "plain", "port": 9092, "type": "internal", "tls": false}
                            ]}}
                        }),
                    )
                } else if path.contains("/secrets/") {
                    (
                        404,
                        json!({
                            "kind": "Status", "apiVersion": "v1", "metadata": {},
                            "status": "Failure", "message": "secret not found",
                            "reason": "NotFound", "code": 404
                        }),
                    )
                } else if path.ends_with("/jobs") {
                    (
                        200,
                        json!({"kind": "JobList", "apiVersion": "batch/v1", "metadata": {}, "items": []}),
                    )
                } else if path.contains("/kafkabackups/") {
                    (200, backup_json.clone())
                } else {
                    // Server-side apply of ConfigMap/CronJob: echo the object back
                    (200, body.clone())
                };

                recorded.lock().unwrap().push(RecordedRequest {
                    method,
                    path,
                    query,
                    body,
                });

                let response = Response::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&response_body).unwrap()))
                    .unwrap();
                send.send_response(response);
            }
        })
    };

    let client = Client::new(mock_service, "kafka");
    let metrics = MetricsState::new();
    reconcile_backup(Arc::new(backup), client, &metrics)
        .await
        .expect("reconcile should succeed");

    dispatcher.await.unwrap();
    Arc::try_unwrap(recorded).unwrap().into_inner().unwrap()
}

fn find_cronjob_patch(requests: &[RecordedRequest]) -> &RecordedRequest {
    requests
        .iter()
        .find(|r| r.method == "PATCH" && r.path.ends_with("/cronjobs/daily-backup-scheduled"))
        .expect("reconcile must apply the CronJob whenever a schedule is set")
}

#[tokio::test]
async fn test_reconcile_propagates_suspend_to_cronjob() {
    let requests = reconcile_with_mock_api(scheduled_backup(true)).await;

    // The CronJob must still be applied when suspended, otherwise a
    // previously-created CronJob keeps firing on its old schedule.
    let cronjob_patch = find_cronjob_patch(&requests);
    assert_eq!(cronjob_patch.body["spec"]["suspend"], json!(true));

    let status_patch = requests
        .iter()
        .find(|r| r.method == "PATCH" && r.path.ends_with("/kafkabackups/daily-backup/status"))
        .expect("reconcile must update the KafkaBackup status");
    assert_eq!(
        status_patch.body["status"]["conditions"][0]["reason"],
        json!("BackupSuspended")
    );
    let status = status_patch.body["status"].as_object().unwrap();
    assert!(
        status.contains_key("nextScheduledBackup") && status["nextScheduledBackup"].is_null(),
        "suspending must clear nextScheduledBackup with an explicit null"
    );
}

#[tokio::test]
async fn test_reconcile_applies_active_cronjob_when_not_suspended() {
    let requests = reconcile_with_mock_api(scheduled_backup(false)).await;

    let cronjob_patch = find_cronjob_patch(&requests);
    assert_eq!(cronjob_patch.body["spec"]["suspend"], json!(false));

    let status_patch = requests
        .iter()
        .find(|r| r.method == "PATCH" && r.path.ends_with("/kafkabackups/daily-backup/status"))
        .expect("reconcile must update the KafkaBackup status");
    assert_eq!(
        status_patch.body["status"]["conditions"][0]["reason"],
        json!("BackupScheduled")
    );
}

#[tokio::test]
async fn test_reconcile_force_applies_resource_updates_to_owned_cronjob() {
    let mut backup = scheduled_backup(false);
    backup.spec.resources = Some(ResourceRequirementsSpec {
        requests: BTreeMap::from([
            ("cpu".to_string(), "250m".to_string()),
            ("memory".to_string(), "256Mi".to_string()),
        ]),
        limits: BTreeMap::from([
            ("cpu".to_string(), "1".to_string()),
            ("memory".to_string(), "1Gi".to_string()),
        ]),
    });

    let requests = reconcile_with_mock_api(backup).await;
    let cronjob_patch = find_cronjob_patch(&requests);
    let resources = &cronjob_patch.body["spec"]["jobTemplate"]["spec"]["template"]["spec"]
        ["containers"][0]["resources"];

    assert_eq!(resources["requests"]["cpu"], json!("250m"));
    assert_eq!(resources["requests"]["memory"], json!("256Mi"));
    assert_eq!(resources["limits"]["cpu"], json!("1"));
    assert_eq!(resources["limits"]["memory"], json!("1Gi"));
    assert!(
        cronjob_patch
            .query
            .as_deref()
            .is_some_and(|query| query.split('&').any(|part| part == "force=true")),
        "controller-owned CronJob fields must be force-applied so a stale field manager cannot block spec updates"
    );
}
