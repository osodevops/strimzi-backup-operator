pub mod backup;
pub mod restore;

/// Finalizer name used by this operator
pub const FINALIZER: &str = "kafkabackup.com/cleanup";

/// Annotation key for manual backup triggers
pub const TRIGGER_ANNOTATION: &str = "kafkabackup.com/trigger";
pub const TRIGGER_VALUE_NOW: &str = "now";

/// Default backup image. Pinned to a public, current kafka-backup release so
/// backup/restore job behavior is deterministic and the image is anonymously
/// pullable by Kubernetes.
pub const DEFAULT_BACKUP_IMAGE: &str = "osodevops/kafka-backup:v0.15.3";

/// Environment variable used by the Helm chart to pass the service account that
/// backup/restore job pods should run as.
pub const JOB_SERVICE_ACCOUNT_ENV: &str = "BACKUP_JOB_SERVICE_ACCOUNT";

/// Fallback used outside Helm. The chart sets `BACKUP_JOB_SERVICE_ACCOUNT` to
/// its rendered service account name, which handles custom release names and
/// `serviceAccount.name` overrides.
pub const DEFAULT_JOB_SERVICE_ACCOUNT: &str = "strimzi-backup-operator";

pub fn job_service_account_name() -> Option<String> {
    let value = std::env::var(JOB_SERVICE_ACCOUNT_ENV)
        .unwrap_or_else(|_| DEFAULT_JOB_SERVICE_ACCOUNT.to_string());
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}
