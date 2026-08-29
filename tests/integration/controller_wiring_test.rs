//! The controllers' watch wiring and the post-start-up resync (issue #62),
//! exercised against a mock API server that answers LISTs and holds WATCHes
//! open.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::pending;
use futures::FutureExt;
use http::{Request, Response};
use http_body_util::BodyExt;
use kafka_backup_operator::controllers::{backup, restore, RunOptions};
use kafka_backup_operator::crd::common::*;
use kafka_backup_operator::crd::kafka_backup::*;
use kafka_backup_operator::crd::KafkaBackup;
use kafka_backup_operator::metrics::prometheus::MetricsState;
use kafka_backup_operator::shutdown::Shutdown;
use kube::client::Body;
use kube::Client;
use serde_json::{json, Value};
use tower_test::mock;

#[derive(Debug, Clone)]
struct Recorded {
    method: String,
    path: String,
    query: String,
}

fn scheduled_backup() -> Value {
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
            suspend: false,
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
    backup.metadata.resource_version = Some("100".to_string());
    backup.metadata.finalizers = Some(vec!["kafkabackup.com/cleanup".to_string()]);
    serde_json::to_value(backup).unwrap()
}

fn never_shutdown() -> Shutdown {
    pending::<()>().boxed().shared()
}

/// Start a mock API server. LISTs answer with `items` for the KafkaBackup
/// collection and empty lists otherwise; WATCHes are held open; everything the
/// reconciler needs (Kafka CR, secrets, jobs, SSA patches, status) is answered
/// like in `reconcile_backup_test`.
fn start_mock(items: Vec<Value>) -> (Client, Arc<Mutex<Vec<Recorded>>>) {
    let (service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let log = Arc::clone(&recorded);
    tokio::spawn(async move {
        let mut held = Vec::new();
        while let Some((request, send)) = handle.next_request().await {
            let method = request.method().to_string();
            let path = request.uri().path().to_string();
            let query = request.uri().query().unwrap_or("").to_string();
            let bytes = request.into_body().collect().await.unwrap().to_bytes();
            let body: Value = if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes).unwrap_or(Value::Null)
            };
            log.lock().unwrap().push(Recorded {
                method: method.clone(),
                path: path.clone(),
                query: query.clone(),
            });

            if query.contains("watch=true") {
                held.push(send);
                continue;
            }
            let (status, response) = if method == "GET" && path.ends_with("/kafkabackups") {
                (
                    200,
                    json!({"kind": "KafkaBackupList", "apiVersion": "kafkabackup.com/v1alpha1",
                           "metadata": {"resourceVersion": "100"}, "items": items}),
                )
            } else if method == "GET"
                && (path.ends_with("/jobs")
                    || path.ends_with("/cronjobs")
                    || path.ends_with("/kafkarestores"))
            {
                (
                    200,
                    json!({"kind": "List", "apiVersion": "v1", "metadata": {"resourceVersion": "1"}, "items": []}),
                )
            } else if path.ends_with("/kafkas/production-cluster") {
                (
                    200,
                    json!({"apiVersion": "kafka.strimzi.io/v1beta2", "kind": "Kafka",
                           "metadata": {"name": "production-cluster", "namespace": "kafka"},
                           "spec": {"kafka": {"replicas": 3, "listeners": [
                               {"name": "plain", "port": 9092, "type": "internal", "tls": false}]}}}),
                )
            } else if path.contains("/secrets/") {
                (
                    404,
                    json!({"kind": "Status", "apiVersion": "v1", "metadata": {}, "status": "Failure",
                           "message": "secret not found", "reason": "NotFound", "code": 404}),
                )
            } else if path.contains("/kafkabackups/daily-backup") && method == "GET" {
                (200, items.first().cloned().unwrap_or(Value::Null))
            } else {
                (200, body)
            };
            let response = Response::builder()
                .status(status)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&response).unwrap()))
                .unwrap();
            send.send_response(response);
        }
    });
    (Client::new(service, "kafka"), recorded)
}

fn list_query(recorded: &[Recorded], path_suffix: &str) -> Option<String> {
    recorded
        .iter()
        .find(|r| {
            r.method == "GET" && r.path.ends_with(path_suffix) && !r.query.contains("watch=true")
        })
        .map(|r| r.query.clone())
}

fn cronjob_patches(recorded: &[Recorded]) -> usize {
    recorded
        .iter()
        .filter(|r| r.method == "PATCH" && r.path.ends_with("/cronjobs/daily-backup-scheduled"))
        .count()
}

#[tokio::test]
async fn backup_controller_watches_owned_jobs_and_cronjobs_by_label() {
    let (client, recorded) = start_mock(vec![]);
    static DELAYS: [Duration; 0] = [];
    let task = tokio::spawn(backup::run_with(
        client,
        Arc::new(MetricsState::new()),
        never_shutdown(),
        RunOptions {
            startup_resync_delays: &DELAYS,
        },
    ));
    tokio::time::sleep(Duration::from_millis(700)).await;
    task.abort();

    let recorded = recorded.lock().unwrap().clone();
    let selector = "labelSelector=kafkabackup.com%2Ftype%3Dbackup";
    let jobs = list_query(&recorded, "/apis/batch/v1/jobs").expect("jobs LIST");
    assert!(jobs.contains(selector), "jobs list query: {jobs}");
    let cronjobs = list_query(&recorded, "/apis/batch/v1/cronjobs").expect("cronjobs LIST");
    assert!(
        cronjobs.contains(selector),
        "cronjobs list query: {cronjobs}"
    );
    assert!(list_query(&recorded, "/apis/kafkabackup.com/v1alpha1/kafkabackups").is_some());
}

#[tokio::test]
async fn restore_controller_watches_owned_jobs_by_label() {
    let (client, recorded) = start_mock(vec![]);
    static DELAYS: [Duration; 0] = [];
    let task = tokio::spawn(restore::run_with(
        client,
        Arc::new(MetricsState::new()),
        never_shutdown(),
        RunOptions {
            startup_resync_delays: &DELAYS,
        },
    ));
    tokio::time::sleep(Duration::from_millis(700)).await;
    task.abort();

    let recorded = recorded.lock().unwrap().clone();
    let jobs = list_query(&recorded, "/apis/batch/v1/jobs").expect("jobs LIST");
    assert!(
        jobs.contains("labelSelector=kafkabackup.com%2Ftype%3Drestore"),
        "{jobs}"
    );
    assert!(
        list_query(&recorded, "/apis/batch/v1/cronjobs").is_none(),
        "the restore controller owns no CronJobs"
    );
}

#[tokio::test]
async fn backup_controller_reconciles_everything_again_after_the_startup_delay() {
    let (client, recorded) = start_mock(vec![scheduled_backup()]);
    static DELAYS: [Duration; 1] = [Duration::from_millis(300)];
    let task = tokio::spawn(backup::run_with(
        client,
        Arc::new(MetricsState::new()),
        never_shutdown(),
        RunOptions {
            startup_resync_delays: &DELAYS,
        },
    ));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    task.abort();

    let recorded = recorded.lock().unwrap().clone();
    let patches = cronjob_patches(&recorded);
    assert!(
        patches >= 2,
        "expected the initial reconcile plus the post-start-up resync, got {patches} CronJob applies"
    );
}

#[tokio::test]
async fn without_a_startup_tick_the_cronjob_is_applied_once() {
    let (client, recorded) = start_mock(vec![scheduled_backup()]);
    static DELAYS: [Duration; 1] = [Duration::from_secs(30)];
    let task = tokio::spawn(backup::run_with(
        client,
        Arc::new(MetricsState::new()),
        never_shutdown(),
        RunOptions {
            startup_resync_delays: &DELAYS,
        },
    ));
    tokio::time::sleep(Duration::from_millis(1500)).await;
    task.abort();

    let recorded = recorded.lock().unwrap().clone();
    assert_eq!(cronjob_patches(&recorded), 1);
}
