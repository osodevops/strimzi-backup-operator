use std::sync::{Arc, Mutex};

use http::{Request, Response};
use http_body_util::BodyExt;
use kafka_backup_operator::crd::common::{
    FilesystemStorageSpec, StorageSpec, StorageType, StrimziClusterRef,
};
use kafka_backup_operator::crd::kafka_backup::KafkaBackupSpec;
use kafka_backup_operator::crd::kafka_restore::{BackupRef, KafkaRestoreSpec};
use kafka_backup_operator::crd::{KafkaBackup, KafkaRestore};
use kafka_backup_operator::metrics::prometheus::MetricsState;
use kafka_backup_operator::reconcilers::backup::reconcile_backup;
use kafka_backup_operator::reconcilers::restore::reconcile_restore;
use kube::client::Body;
use kube::Client;
use serde::Serialize;
use serde_json::json;
use tower_test::mock;

#[derive(Debug)]
struct RecordedRequest {
    method: String,
    path: String,
    body: serde_json::Value,
}

fn cluster_ref() -> StrimziClusterRef {
    StrimziClusterRef {
        name: "missing-after-disaster".to_string(),
        namespace: None,
        ca_secret: None,
        listener: None,
    }
}

fn paused_backup() -> KafkaBackup {
    let spec = KafkaBackupSpec {
        strimzi_cluster_ref: cluster_ref(),
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
        schedule: None,
        retention: None,
        resources: None,
        template: None,
        image: None,
        backoff_limit: None,
    };
    let mut backup = KafkaBackup::new("restore-source", spec);
    backup.metadata.namespace = Some("kafka".to_string());
    backup.metadata.uid = Some("backup-uid".to_string());
    backup.metadata.generation = Some(1);
    backup.metadata.annotations = Some(std::collections::BTreeMap::from([(
        "strimzi.io/pause-reconciliation".to_string(),
        "true".to_string(),
    )]));
    backup
}

fn paused_restore() -> KafkaRestore {
    let spec = KafkaRestoreSpec {
        strimzi_cluster_ref: cluster_ref(),
        authentication: None,
        backup_ref: BackupRef {
            name: "restore-source".to_string(),
            backup_id: Some("snapshot-before-disaster".to_string()),
        },
        topics: None,
        point_in_time: None,
        connection: None,
        logging: None,
        env: Vec::new(),
        topic_mapping: Vec::new(),
        consumer_groups: None,
        restore: None,
        metrics: None,
        resources: None,
        template: None,
        image: None,
        backoff_limit: None,
    };
    let mut restore = KafkaRestore::new("disaster-restore", spec);
    restore.metadata.namespace = Some("kafka".to_string());
    restore.metadata.uid = Some("restore-uid".to_string());
    restore.metadata.generation = Some(1);
    restore.metadata.annotations = Some(std::collections::BTreeMap::from([(
        "strimzi.io/pause-reconciliation".to_string(),
        "TRUE".to_string(),
    )]));
    restore
}

async fn reconcile_paused_resource<K, F, Fut>(resource: K, reconcile: F) -> Vec<RecordedRequest>
where
    K: Serialize,
    F: FnOnce(Client, MetricsState) -> Fut,
    Fut: std::future::Future<Output = kafka_backup_operator::error::Result<()>>,
{
    let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    let resource_json = serde_json::to_value(resource).unwrap();

    let dispatcher = {
        let recorded = Arc::clone(&recorded);
        tokio::spawn(async move {
            while let Some((request, send)) = handle.next_request().await {
                let method = request.method().to_string();
                let path = request.uri().path().to_string();
                let bytes = request.into_body().collect().await.unwrap().to_bytes();
                let body = if bytes.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_slice(&bytes).unwrap()
                };

                recorded
                    .lock()
                    .unwrap()
                    .push(RecordedRequest { method, path, body });

                let response = Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&resource_json).unwrap()))
                    .unwrap();
                send.send_response(response);
            }
        })
    };

    let client = Client::new(mock_service, "kafka");
    reconcile(client, MetricsState::new())
        .await
        .expect("paused reconciliation should succeed");
    dispatcher.await.unwrap();
    Arc::try_unwrap(recorded).unwrap().into_inner().unwrap()
}

fn assert_status_only(requests: &[RecordedRequest], resource_plural: &str, name: &str) {
    assert_eq!(requests.len(), 1, "a paused resource must be status-only");
    let request = &requests[0];
    assert_eq!(request.method, "PATCH");
    assert!(request
        .path
        .ends_with(&format!("/{resource_plural}/{name}/status")));
    assert_eq!(
        request.body["status"]["conditions"][0]["type"],
        json!("ReconciliationPaused")
    );
    assert_eq!(
        request.body["status"]["conditions"][0]["status"],
        json!("True")
    );
    assert_eq!(request.body["status"]["observedGeneration"], json!(1));
}

#[tokio::test]
async fn paused_backup_does_not_create_operator_resources() {
    let backup = paused_backup();
    let requests = reconcile_paused_resource(backup.clone(), move |client, metrics| async move {
        reconcile_backup(Arc::new(backup), client, &metrics).await
    })
    .await;

    assert_status_only(&requests, "kafkabackups", "restore-source");
}

#[tokio::test]
async fn paused_restore_does_not_resolve_dependencies_or_create_a_job() {
    let restore = paused_restore();
    let requests = reconcile_paused_resource(restore.clone(), move |client, metrics| async move {
        reconcile_restore(Arc::new(restore), client, &metrics).await
    })
    .await;

    assert_status_only(&requests, "kafkarestores", "disaster-restore");
}
