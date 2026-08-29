//! Health, readiness and metrics HTTP endpoints.

use std::sync::Arc;

use axum::{routing::get, Router};
use tokio::sync::watch;

use crate::leader::{readiness, LeaderState};
use crate::metrics::prometheus::MetricsState;

/// Build the operator's HTTP router.
///
/// * `/healthz` — process liveness, always `200 ok`.
/// * `/readyz` — see [`readiness`]: `503` until this replica has observed the
///   leader lease at least once, `200 standby` / `200 leader` afterwards. With
///   leader election disabled the state starts at `Leader`, so the endpoint is
///   ready as soon as the server is up (the previous behaviour).
/// * `/metrics` — Prometheus text exposition.
pub fn router(metrics: Arc<MetricsState>, state_rx: watch::Receiver<LeaderState>) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route(
            "/readyz",
            get(move || {
                let state_rx = state_rx.clone();
                async move { readiness(*state_rx.borrow()) }
            }),
        )
        .route(
            "/metrics",
            get(move || {
                let state = Arc::clone(&metrics);
                async move {
                    (
                        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
                        state.gather(),
                    )
                }
            }),
        )
}
