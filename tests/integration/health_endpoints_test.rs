//! `/healthz`, `/readyz` and `/metrics` behaviour under leader election.

use std::sync::Arc;

use http::{Request, StatusCode};
use http_body_util::BodyExt;
use kafka_backup_operator::leader::LeaderState;
use kafka_backup_operator::metrics::prometheus::MetricsState;
use kafka_backup_operator::server::router;
use tokio::sync::watch;
use tower::ServiceExt;

async fn get(
    metrics: &Arc<MetricsState>,
    rx: &watch::Receiver<LeaderState>,
    path: &str,
) -> (StatusCode, String) {
    let app = router(Arc::clone(metrics), rx.clone());
    let response = app
        .oneshot(
            Request::builder()
                .uri(path)
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

#[tokio::test]
async fn healthz_is_always_ok() {
    let metrics = Arc::new(MetricsState::new());
    let (_tx, rx) = watch::channel(LeaderState::Unknown);
    assert_eq!(
        get(&metrics, &rx, "/healthz").await,
        (StatusCode::OK, "ok".into())
    );
}

#[tokio::test]
async fn readyz_is_unavailable_until_the_lease_has_been_observed() {
    let metrics = Arc::new(MetricsState::new());
    let (tx, rx) = watch::channel(LeaderState::Unknown);
    let (status, body) = get(&metrics, &rx, "/readyz").await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(body, "leader election pending");

    tx.send(LeaderState::Follower).unwrap();
    assert_eq!(
        get(&metrics, &rx, "/readyz").await,
        (StatusCode::OK, "standby".into())
    );

    tx.send(LeaderState::Leader).unwrap();
    assert_eq!(
        get(&metrics, &rx, "/readyz").await,
        (StatusCode::OK, "leader".into())
    );

    // A leader that lost the lease goes back to standby, still ready.
    tx.send(LeaderState::Follower).unwrap();
    assert_eq!(get(&metrics, &rx, "/readyz").await.0, StatusCode::OK);
}

#[tokio::test]
async fn readyz_is_ok_from_the_start_when_election_is_disabled() {
    let metrics = Arc::new(MetricsState::new());
    let (_tx, rx) = watch::channel(LeaderState::Leader);
    assert_eq!(
        get(&metrics, &rx, "/readyz").await,
        (StatusCode::OK, "leader".into())
    );
}

#[tokio::test]
async fn metrics_expose_the_leader_gauge() {
    let metrics = Arc::new(MetricsState::new());
    let (_tx, rx) = watch::channel(LeaderState::Leader);
    metrics.set_leader("sbo-operator-abc", false);
    let (status, body) = get(&metrics, &rx, "/metrics").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        body.contains("strimzi_backup_operator_leader{identity=\"sbo-operator-abc\"} 0"),
        "{body}"
    );
    metrics.set_leader("sbo-operator-abc", true);
    let (_, body) = get(&metrics, &rx, "/metrics").await;
    assert!(body.contains("strimzi_backup_operator_leader{identity=\"sbo-operator-abc\"} 1"));
}
