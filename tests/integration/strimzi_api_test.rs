use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use http::{Request, Response};
use http_body_util::BodyExt;
use kafka_backup_operator::crd::common::{
    AuthenticationSpec, AuthenticationType, KafkaUserRef, StrimziClusterRef,
};
use kafka_backup_operator::strimzi::kafka_cr::resolve_kafka_cluster;
use kafka_backup_operator::strimzi::kafka_user::{resolve_auth, ResolvedAuth};
use kube::{client::Body, Client};
use serde_json::{json, Value};
use tokio::task::JoinHandle;
use tower_test::mock;

const KAFKA_V1_PATH: &str = "/apis/kafka.strimzi.io/v1/namespaces/kafka/kafkas/my-cluster";
const KAFKA_V1BETA2_PATH: &str =
    "/apis/kafka.strimzi.io/v1beta2/namespaces/kafka/kafkas/my-cluster";
const KAFKA_USER_V1_PATH: &str =
    "/apis/kafka.strimzi.io/v1/namespaces/kafka/kafkausers/backup-user";
const KAFKA_USER_V1BETA2_PATH: &str =
    "/apis/kafka.strimzi.io/v1beta2/namespaces/kafka/kafkausers/backup-user";

struct MockApi {
    client: Client,
    requests: Arc<Mutex<Vec<String>>>,
    dispatcher: JoinHandle<()>,
}

impl MockApi {
    fn new(responses: impl IntoIterator<Item = (&'static str, u16, Value)>) -> Self {
        let responses: HashMap<String, (u16, Value)> = responses
            .into_iter()
            .map(|(path, status, body)| (path.to_string(), (status, body)))
            .collect();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let (mock_service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();

        let dispatcher = tokio::spawn(async move {
            while let Some((request, send)) = handle.next_request().await {
                let path = request.uri().path().to_string();
                request.into_body().collect().await.unwrap();
                recorded.lock().unwrap().push(path.clone());

                let (status, body) = responses
                    .get(&path)
                    .cloned()
                    .unwrap_or_else(|| (404, not_found(&path)));
                let response = Response::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap();
                send.send_response(response);
            }
        });

        Self {
            client: Client::new(mock_service, "kafka"),
            requests,
            dispatcher,
        }
    }

    async fn finish(self) -> Vec<String> {
        drop(self.client);
        self.dispatcher.await.unwrap();
        Arc::try_unwrap(self.requests)
            .unwrap()
            .into_inner()
            .unwrap()
    }
}

fn not_found(path: &str) -> Value {
    json!({
        "kind": "Status",
        "apiVersion": "v1",
        "metadata": {},
        "status": "Failure",
        "message": format!("resource at {path} not found"),
        "reason": "NotFound",
        "code": 404
    })
}

fn forbidden(path: &str) -> Value {
    json!({
        "kind": "Status",
        "apiVersion": "v1",
        "metadata": {},
        "status": "Failure",
        "message": format!("access to {path} is forbidden"),
        "reason": "Forbidden",
        "code": 403
    })
}

fn kafka(api_version: &str) -> Value {
    json!({
        "apiVersion": api_version,
        "kind": "Kafka",
        "metadata": {"name": "my-cluster", "namespace": "kafka"},
        "spec": {"kafka": {"listeners": [
            {"name": "plain", "port": 9092, "type": "internal", "tls": false}
        ]}},
        "status": {"listeners": [
            {"name": "plain", "bootstrapServers": "my-cluster-kafka-bootstrap.kafka.svc:9092"}
        ]}
    })
}

fn kafka_user(api_version: &str) -> Value {
    json!({
        "apiVersion": api_version,
        "kind": "KafkaUser",
        "metadata": {"name": "backup-user", "namespace": "kafka"},
        "status": {"secret": "backup-user-credentials"}
    })
}

fn cluster_ref() -> StrimziClusterRef {
    StrimziClusterRef {
        name: "my-cluster".to_string(),
        namespace: None,
        ca_secret: None,
        listener: None,
    }
}

fn scram_auth() -> AuthenticationSpec {
    AuthenticationSpec {
        auth_type: AuthenticationType::ScramSha512,
        kafka_user_ref: Some(KafkaUserRef {
            name: "backup-user".to_string(),
        }),
        certificate_and_key: None,
        password_secret: None,
        username: None,
    }
}

#[tokio::test]
async fn resolves_kafka_from_strimzi_v1() {
    let api = MockApi::new([(KAFKA_V1_PATH, 200, kafka("kafka.strimzi.io/v1"))]);

    let cluster = resolve_kafka_cluster(&api.client, &cluster_ref(), "kafka", None)
        .await
        .expect("Strimzi 1.0 serves Kafka only at v1");

    assert_eq!(cluster.name, "my-cluster");
    assert_eq!(
        cluster.bootstrap_servers,
        "my-cluster-kafka-bootstrap.kafka.svc:9092"
    );
    assert_eq!(api.finish().await, [KAFKA_V1_PATH]);
}

#[tokio::test]
async fn falls_back_to_v1beta2_for_older_strimzi_kafka() {
    let api = MockApi::new([
        (KAFKA_V1_PATH, 404, not_found(KAFKA_V1_PATH)),
        (KAFKA_V1BETA2_PATH, 200, kafka("kafka.strimzi.io/v1beta2")),
    ]);

    resolve_kafka_cluster(&api.client, &cluster_ref(), "kafka", None)
        .await
        .expect("pre-0.49 Strimzi Kafka resources must remain supported");

    assert_eq!(api.finish().await, [KAFKA_V1_PATH, KAFKA_V1BETA2_PATH]);
}

#[tokio::test]
async fn kafka_missing_from_both_versions_keeps_domain_error() {
    let api = MockApi::new([
        (KAFKA_V1_PATH, 404, not_found(KAFKA_V1_PATH)),
        (KAFKA_V1BETA2_PATH, 404, not_found(KAFKA_V1BETA2_PATH)),
    ]);

    let error = resolve_kafka_cluster(&api.client, &cluster_ref(), "kafka", None)
        .await
        .unwrap_err();

    assert_eq!(error.reason(), "StrimziClusterNotFound");
    assert_eq!(api.finish().await, [KAFKA_V1_PATH, KAFKA_V1BETA2_PATH]);
}

#[tokio::test]
async fn kafka_authorization_error_is_not_masked_by_fallback() {
    let api = MockApi::new([
        (KAFKA_V1_PATH, 403, forbidden(KAFKA_V1_PATH)),
        (KAFKA_V1BETA2_PATH, 200, kafka("kafka.strimzi.io/v1beta2")),
    ]);

    let error = resolve_kafka_cluster(&api.client, &cluster_ref(), "kafka", None)
        .await
        .unwrap_err();

    assert_eq!(error.reason(), "KubernetesError");
    assert_eq!(api.finish().await, [KAFKA_V1_PATH]);
}

#[tokio::test]
async fn resolves_kafka_user_from_strimzi_v1() {
    let api = MockApi::new([(KAFKA_USER_V1_PATH, 200, kafka_user("kafka.strimzi.io/v1"))]);

    let auth = resolve_auth(&api.client, Some(&scram_auth()), "kafka")
        .await
        .expect("Strimzi 1.0 serves KafkaUser only at v1");

    match auth {
        ResolvedAuth::ScramSha512 {
            username,
            secret_name,
            password_key,
        } => {
            assert_eq!(username, "backup-user");
            assert_eq!(secret_name, "backup-user-credentials");
            assert_eq!(password_key, "password");
        }
        other => panic!("expected SCRAM authentication, got {other:?}"),
    }
    assert_eq!(api.finish().await, [KAFKA_USER_V1_PATH]);
}

#[tokio::test]
async fn falls_back_to_v1beta2_for_older_strimzi_kafka_user() {
    let api = MockApi::new([
        (KAFKA_USER_V1_PATH, 404, not_found(KAFKA_USER_V1_PATH)),
        (
            KAFKA_USER_V1BETA2_PATH,
            200,
            kafka_user("kafka.strimzi.io/v1beta2"),
        ),
    ]);

    resolve_auth(&api.client, Some(&scram_auth()), "kafka")
        .await
        .expect("pre-0.49 Strimzi KafkaUser resources must remain supported");

    assert_eq!(
        api.finish().await,
        [KAFKA_USER_V1_PATH, KAFKA_USER_V1BETA2_PATH]
    );
}

#[tokio::test]
async fn kafka_user_missing_from_both_versions_keeps_domain_error() {
    let api = MockApi::new([
        (KAFKA_USER_V1_PATH, 404, not_found(KAFKA_USER_V1_PATH)),
        (
            KAFKA_USER_V1BETA2_PATH,
            404,
            not_found(KAFKA_USER_V1BETA2_PATH),
        ),
    ]);

    let error = resolve_auth(&api.client, Some(&scram_auth()), "kafka")
        .await
        .unwrap_err();

    assert_eq!(error.reason(), "KafkaUserNotFound");
    assert_eq!(
        api.finish().await,
        [KAFKA_USER_V1_PATH, KAFKA_USER_V1BETA2_PATH]
    );
}
