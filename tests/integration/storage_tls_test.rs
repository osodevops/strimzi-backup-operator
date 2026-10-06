//! Issue #76: the operator's own storage client — retention discovery and
//! pruning — must trust `spec.storage.tls.trustedCertificates`, not only the
//! Job pods. A fake S3 endpoint serves HTTPS with a certificate signed by a
//! private CA generated per test; the Secrets come from a mock API server.

use std::{
    collections::{BTreeMap, HashMap},
    net::{IpAddr, Ipv4Addr},
    sync::{Arc, Mutex},
    time::Duration,
};

use http::{Request, Response};
use http_body_util::BodyExt;
use k8s_openapi::{api::core::v1::Secret, ByteString};
use kafka_backup_operator::crd::common::{
    CertSecretSource, S3StorageSpec, SecretKeyRef, StorageSpec, StorageTlsSpec, StorageType,
};
use kafka_backup_operator::error::Error;
use kafka_backup_operator::retention::storage::discover_backup_history;
use kube::{api::ObjectMeta, client::Body, Client};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose, SanType,
};
use serde_json::{json, Value};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};
use tokio_rustls::{rustls, TlsAcceptor};
use tower_test::mock;

const LIST_RESPONSE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<ListBucketResult xmlns="http://s3.amazonaws.com/doc/2006-03-01/"><Name>kafka-backups</Name><Prefix>issue76/</Prefix><KeyCount>0</KeyCount><MaxKeys>1000</MaxKeys><IsTruncated>false</IsTruncated></ListBucketResult>"#;

/// A private CA and a server certificate it signed for `127.0.0.1`.
struct PrivatePki {
    ca_pem: String,
    server_der: Vec<u8>,
    server_key_der: Vec<u8>,
}

fn private_pki() -> PrivatePki {
    let mut ca = CertificateParams::new(Vec::<String>::new()).unwrap();
    ca.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    ca.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    ca.distinguished_name
        .push(DnType::CommonName, "issue76 private test CA");
    let ca = CertifiedIssuer::self_signed(ca, KeyPair::generate().unwrap()).unwrap();

    let mut server = CertificateParams::new(vec!["localhost".to_string()]).unwrap();
    server
        .subject_alt_names
        .push(SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)));
    server.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let server_key = KeyPair::generate().unwrap();
    let server_cert = server.signed_by(&server_key, &ca).unwrap();

    PrivatePki {
        ca_pem: ca.pem(),
        server_der: server_cert.der().to_vec(),
        server_key_der: server_key.serialize_der(),
    }
}

/// An HTTPS S3 endpoint that answers every request with an empty listing and
/// reports the TLS handshakes it saw fail.
struct FakeS3 {
    endpoint: String,
    requests: Arc<Mutex<Vec<String>>>,
    handshake_errors: mpsc::UnboundedReceiver<String>,
}

async fn fake_s3(pki: &PrivatePki) -> FakeS3 {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(
            vec![pki.server_der.clone().into()],
            rustls::pki_types::PrivateKeyDer::Pkcs8(pki.server_key_der.clone().into()),
        )
        .unwrap();
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("https://{}", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let (errors_tx, handshake_errors) = mpsc::unbounded_channel();

    let recorded = Arc::clone(&requests);
    tokio::spawn(async move {
        while let Ok((tcp, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            let recorded = Arc::clone(&recorded);
            let errors_tx = errors_tx.clone();
            tokio::spawn(async move {
                let mut tls = match acceptor.accept(tcp).await {
                    Ok(tls) => tls,
                    Err(e) => {
                        let _ = errors_tx.send(e.to_string());
                        return;
                    }
                };
                let mut head = Vec::new();
                let mut buf = [0u8; 4096];
                while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                    match tls.read(&mut buf).await {
                        Ok(0) | Err(_) => return,
                        Ok(n) => head.extend_from_slice(&buf[..n]),
                    }
                }
                let request_line = String::from_utf8_lossy(&head)
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string();
                recorded.lock().unwrap().push(request_line);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/xml\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{LIST_RESPONSE}",
                    LIST_RESPONSE.len()
                );
                let _ = tls.write_all(response.as_bytes()).await;
                let _ = tls.shutdown().await;
            });
        }
    });

    FakeS3 {
        endpoint,
        requests,
        handshake_errors,
    }
}

fn secret(name: &str, data: &[(&str, &str)]) -> Value {
    let data: BTreeMap<String, ByteString> = data
        .iter()
        .map(|(k, v)| (k.to_string(), ByteString(v.as_bytes().to_vec())))
        .collect();
    serde_json::to_value(Secret {
        metadata: ObjectMeta {
            name: Some(name.to_string()),
            namespace: Some("kafka".to_string()),
            ..Default::default()
        },
        data: Some(data),
        ..Default::default()
    })
    .unwrap()
}

/// A mock API server that serves the given Secrets in namespace `kafka`.
fn mock_client(secrets: Vec<Value>) -> Client {
    let by_path: HashMap<String, Value> = secrets
        .into_iter()
        .map(|s| {
            let name = s["metadata"]["name"].as_str().unwrap().to_string();
            (format!("/api/v1/namespaces/kafka/secrets/{name}"), s)
        })
        .collect();
    let (service, mut handle) = mock::pair::<Request<Body>, Response<Body>>();
    tokio::spawn(async move {
        while let Some((request, send)) = handle.next_request().await {
            let path = request.uri().path().to_string();
            request.into_body().collect().await.unwrap();
            let (status, body) = match by_path.get(&path) {
                Some(secret) => (200, secret.clone()),
                None => (
                    404,
                    json!({"kind": "Status", "apiVersion": "v1", "metadata": {},
                           "status": "Failure", "reason": "NotFound", "code": 404,
                           "message": format!("{path} not found")}),
                ),
            };
            send.send_response(
                Response::builder()
                    .status(status)
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&body).unwrap()))
                    .unwrap(),
            );
        }
    });
    Client::new(service, "kafka")
}

fn credentials_secret() -> Value {
    secret(
        "minio-credentials",
        &[
            ("access-key-id", "minioadmin"),
            ("secret-access-key", "minioadmin"),
        ],
    )
}

fn s3_storage(endpoint: &str, tls: Option<StorageTlsSpec>) -> StorageSpec {
    StorageSpec {
        storage_type: StorageType::S3,
        s3: Some(S3StorageSpec {
            bucket: "kafka-backups".to_string(),
            region: Some("us-east-1".to_string()),
            prefix: Some("issue76".to_string()),
            endpoint: Some(endpoint.to_string()),
            force_path_style: Some(true),
            allow_http: None,
            credentials_secret: None,
            access_key_secret: Some(SecretKeyRef {
                name: "minio-credentials".to_string(),
                key: "access-key-id".to_string(),
            }),
            secret_key_secret: Some(SecretKeyRef {
                name: "minio-credentials".to_string(),
                key: "secret-access-key".to_string(),
            }),
        }),
        azure: None,
        gcs: None,
        filesystem: None,
        tls,
    }
}

fn trust(secret_name: &str, certificate: &str) -> Option<StorageTlsSpec> {
    Some(StorageTlsSpec {
        trusted_certificates: vec![CertSecretSource {
            secret_name: secret_name.to_string(),
            certificate: certificate.to_string(),
        }],
    })
}

#[tokio::test]
async fn retention_lists_a_private_ca_endpoint_with_trusted_certificates() {
    let pki = private_pki();
    let s3 = fake_s3(&pki).await;
    let client = mock_client(vec![
        credentials_secret(),
        secret("minio-ca", &[("ca.crt", &pki.ca_pem)]),
    ]);
    let storage = s3_storage(&s3.endpoint, trust("minio-ca", "ca.crt"));

    let history = tokio::time::timeout(
        Duration::from_secs(20),
        discover_backup_history(&client, "kafka", &storage, "issue76"),
    )
    .await
    .expect("listing over TLS must not hang retrying a failed handshake")
    .expect("the private CA from the Secret must be trusted");

    assert!(history.is_empty());
    let requests = s3.requests.lock().unwrap().clone();
    assert!(
        requests
            .iter()
            .any(|r| r.starts_with("GET /kafka-backups?list-type=2")),
        "expected an S3 ListObjectsV2 call, got {requests:?}"
    );
}

/// Negative control: the same endpoint is rejected without the CA, so the
/// test above passes because of `trustedCertificates`, not by accident.
#[tokio::test]
async fn retention_rejects_a_private_ca_endpoint_without_trusted_certificates() {
    let pki = private_pki();
    let mut s3 = fake_s3(&pki).await;
    let client = mock_client(vec![credentials_secret()]);
    let storage = s3_storage(&s3.endpoint, None);

    // The client retries failed handshakes for minutes; watch the server side.
    let discovery = tokio::spawn(async move {
        discover_backup_history(&client, "kafka", &storage, "issue76").await
    });
    let handshake_error = tokio::time::timeout(Duration::from_secs(20), s3.handshake_errors.recv())
        .await
        .expect("the client must attempt a TLS handshake")
        .unwrap();
    discovery.abort();

    assert!(
        handshake_error.contains("UnknownCA") || handshake_error.contains("BadCertificate"),
        "client should reject the private CA, server saw: {handshake_error}"
    );
    assert!(s3.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn retention_reports_a_trusted_certificate_secret_without_pem() {
    let pki = private_pki();
    let s3 = fake_s3(&pki).await;
    let client = mock_client(vec![
        credentials_secret(),
        secret("minio-ca", &[("ca.crt", "not a certificate")]),
    ]);
    let storage = s3_storage(&s3.endpoint, trust("minio-ca", "ca.crt"));

    let err = discover_backup_history(&client, "kafka", &storage, "issue76")
        .await
        .expect_err("a Secret key without a PEM certificate must be rejected");

    match err {
        Error::InvalidConfig(message) => {
            assert!(message.contains("minio-ca"), "{message}");
            assert!(message.contains("ca.crt"), "{message}");
        }
        other => panic!("expected InvalidConfig, got {other:?}"),
    }
    assert!(s3.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn retention_reports_a_missing_trusted_certificate_secret() {
    let pki = private_pki();
    let s3 = fake_s3(&pki).await;
    let client = mock_client(vec![credentials_secret()]);
    let storage = s3_storage(&s3.endpoint, trust("minio-ca", "ca.crt"));

    let err = discover_backup_history(&client, "kafka", &storage, "issue76")
        .await
        .expect_err("a missing CA Secret must be reported");

    assert!(
        matches!(&err, Error::SecretNotFound { name, .. } if name == "minio-ca"),
        "{err:?}"
    );
}
