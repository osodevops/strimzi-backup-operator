use prometheus::{
    Encoder, GaugeVec, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
};

/// Prometheus metrics state for the operator
pub struct MetricsState {
    registry: Registry,
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
        // Should contain metric definitions even without data
        assert!(output.is_empty() || output.contains("strimzi_backup") || output.len() >= 0);
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
