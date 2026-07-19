use prometheus::{
    Encoder, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, IntGaugeVec, Opts, Registry,
    TextEncoder,
};
use std::time::Duration;

/// Prometheus metrics state for the operator
pub struct MetricsState {
    registry: Registry,
    pub operator_build_info: IntGaugeVec,
    pub operator_reconciliations_total: IntCounterVec,
    pub operator_reconciliation_duration_seconds: HistogramVec,
    pub backup_records_total: IntCounterVec,
    pub backup_bytes_total: IntCounterVec,
    pub backup_duration_seconds: HistogramVec,
    pub backup_last_success_timestamp: GaugeVec,
    pub backup_last_failure_timestamp: GaugeVec,
    pub restore_records_total: IntCounterVec,
    pub restore_bytes_total: IntCounterVec,
    pub restore_duration_seconds: HistogramVec,
    pub backup_storage_bytes: GaugeVec,
    pub backup_lag_seconds: GaugeVec,
}

impl Default for MetricsState {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsState {
    pub fn new() -> Self {
        let registry = Registry::new();

        // Keep one always-present series so a healthy, idle operator never
        // serves an empty successful response from /metrics.
        let operator_build_info = IntGaugeVec::new(
            Opts::new(
                "strimzi_backup_operator_build_info",
                "Build information for the Strimzi backup operator",
            ),
            &["version"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(operator_build_info.clone()))
            .expect("metric registration");
        operator_build_info
            .with_label_values(&[env!("CARGO_PKG_VERSION")])
            .set(1);

        let operator_reconciliations_total = IntCounterVec::new(
            Opts::new(
                "strimzi_backup_operator_reconciliations_total",
                "Total custom resource reconciliations handled by the operator",
            ),
            &["controller", "result"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(operator_reconciliations_total.clone()))
            .expect("metric registration");

        let operator_reconciliation_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "strimzi_backup_operator_reconciliation_duration_seconds",
                "Duration of custom resource reconciliations",
            )
            .buckets(vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0]),
            &["controller", "result"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(operator_reconciliation_duration_seconds.clone()))
            .expect("metric registration");

        let backup_records_total = IntCounterVec::new(
            Opts::new(
                "strimzi_backup_records_total",
                "Total number of records backed up",
            ),
            &["backup_name", "cluster"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(backup_records_total.clone()))
            .expect("metric registration");

        let backup_bytes_total = IntCounterVec::new(
            Opts::new(
                "strimzi_backup_bytes_total",
                "Total number of bytes backed up",
            ),
            &["backup_name", "cluster"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(backup_bytes_total.clone()))
            .expect("metric registration");

        let backup_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "strimzi_backup_duration_seconds",
                "Duration of backup operations in seconds",
            )
            .buckets(vec![
                10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1200.0, 1800.0, 3600.0,
            ]),
            &["backup_name", "cluster"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(backup_duration_seconds.clone()))
            .expect("metric registration");

        let backup_last_success_timestamp = GaugeVec::new(
            Opts::new(
                "strimzi_backup_last_success_timestamp",
                "Timestamp of last successful backup (unix epoch)",
            ),
            &["backup_name", "cluster"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(backup_last_success_timestamp.clone()))
            .expect("metric registration");

        let backup_last_failure_timestamp = GaugeVec::new(
            Opts::new(
                "strimzi_backup_last_failure_timestamp",
                "Timestamp of last failed backup (unix epoch)",
            ),
            &["backup_name", "cluster"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(backup_last_failure_timestamp.clone()))
            .expect("metric registration");

        let restore_records_total = IntCounterVec::new(
            Opts::new(
                "strimzi_restore_records_total",
                "Total number of records restored",
            ),
            &["restore_name", "cluster"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(restore_records_total.clone()))
            .expect("metric registration");

        let restore_bytes_total = IntCounterVec::new(
            Opts::new(
                "strimzi_restore_bytes_total",
                "Total number of bytes restored",
            ),
            &["restore_name", "cluster"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(restore_bytes_total.clone()))
            .expect("metric registration");

        let restore_duration_seconds = HistogramVec::new(
            HistogramOpts::new(
                "strimzi_restore_duration_seconds",
                "Duration of restore operations in seconds",
            )
            .buckets(vec![
                10.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1200.0, 1800.0, 3600.0,
            ]),
            &["restore_name", "cluster"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(restore_duration_seconds.clone()))
            .expect("metric registration");

        let backup_storage_bytes = GaugeVec::new(
            Opts::new(
                "strimzi_backup_storage_bytes",
                "Total storage used by backups in bytes",
            ),
            &["backup_name", "cluster"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(backup_storage_bytes.clone()))
            .expect("metric registration");

        let backup_lag_seconds = GaugeVec::new(
            Opts::new(
                "strimzi_backup_lag_seconds",
                "Time since last successful backup in seconds",
            ),
            &["backup_name", "cluster"],
        )
        .expect("metric creation");
        registry
            .register(Box::new(backup_lag_seconds.clone()))
            .expect("metric registration");

        Self {
            registry,
            operator_build_info,
            operator_reconciliations_total,
            operator_reconciliation_duration_seconds,
            backup_records_total,
            backup_bytes_total,
            backup_duration_seconds,
            backup_last_success_timestamp,
            backup_last_failure_timestamp,
            restore_records_total,
            restore_bytes_total,
            restore_duration_seconds,
            backup_storage_bytes,
            backup_lag_seconds,
        }
    }

    /// Gather all metrics and encode as Prometheus text format
    pub fn gather(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }

    /// Record one controller reconciliation and its result.
    pub fn record_reconciliation(&self, controller: &str, succeeded: bool, duration: Duration) {
        let result = if succeeded { "success" } else { "error" };
        let labels = &[controller, result];
        self.operator_reconciliations_total
            .with_label_values(labels)
            .inc();
        self.operator_reconciliation_duration_seconds
            .with_label_values(labels)
            .observe(duration.as_secs_f64());
    }

    /// Record a successful backup completion
    pub fn record_backup_success(
        &self,
        backup_name: &str,
        cluster: &str,
        records: u64,
        bytes: u64,
        duration_secs: f64,
    ) {
        self.backup_records_total
            .with_label_values(&[backup_name, cluster])
            .inc_by(records);
        self.backup_bytes_total
            .with_label_values(&[backup_name, cluster])
            .inc_by(bytes);
        self.backup_duration_seconds
            .with_label_values(&[backup_name, cluster])
            .observe(duration_secs);
        self.backup_last_success_timestamp
            .with_label_values(&[backup_name, cluster])
            .set(chrono::Utc::now().timestamp() as f64);
        self.backup_lag_seconds
            .with_label_values(&[backup_name, cluster])
            .set(0.0);
    }

    /// Record a backup failure
    pub fn record_backup_failure(&self, backup_name: &str, cluster: &str) {
        self.backup_last_failure_timestamp
            .with_label_values(&[backup_name, cluster])
            .set(chrono::Utc::now().timestamp() as f64);
    }

    /// Record a successful restore completion
    pub fn record_restore_success(
        &self,
        restore_name: &str,
        cluster: &str,
        records: u64,
        bytes: u64,
        duration_secs: f64,
    ) {
        self.restore_records_total
            .with_label_values(&[restore_name, cluster])
            .inc_by(records);
        self.restore_bytes_total
            .with_label_values(&[restore_name, cluster])
            .inc_by(bytes);
        self.restore_duration_seconds
            .with_label_values(&[restore_name, cluster])
            .observe(duration_secs);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let state = MetricsState::new();
        let output = state.gather();
        assert!(output.contains("strimzi_backup_operator_build_info"));
        assert!(output.contains(&format!("version=\"{}\"", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn test_record_reconciliation() {
        let state = MetricsState::new();
        state.record_reconciliation("backup", true, Duration::from_millis(125));

        let output = state.gather();
        assert!(output.contains(
            "strimzi_backup_operator_reconciliations_total{controller=\"backup\",result=\"success\"} 1"
        ));
        assert!(output.contains("strimzi_backup_operator_reconciliation_duration_seconds_count"));
    }

    #[test]
    fn test_record_backup_success() {
        let state = MetricsState::new();
        state.record_backup_success("test-backup", "my-cluster", 1000, 1048576, 120.5);

        let output = state.gather();
        assert!(output.contains("strimzi_backup_records_total"));
        assert!(output.contains("strimzi_backup_bytes_total"));
    }
}
